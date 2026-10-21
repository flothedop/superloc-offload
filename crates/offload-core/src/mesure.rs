//! Mesures de débit et imputation du goulot d'étranglement.
//!
//! L'objectif n'est pas d'afficher un chiffre flatteur mais de pouvoir dire,
//! preuve à l'appui, *où* la copie a attendu. Le système accepte les
//! écritures en mémoire : la vitesse instantanée de `write()` ment. Seule
//! une moyenne sur le temps réel, écriture physique comprise, a un sens.

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// Durée de la fenêtre glissante servant au débit affiché.
const FENETRE: Duration = Duration::from_secs(5);
/// Au-delà, une pause est comptée comme un blocage franc.
const SEUIL_BLOCAGE: Duration = Duration::from_millis(1000);

/// Débit instantané, calculé sur une fenêtre glissante de temps réel.
pub struct Debitmetre {
    echantillons: VecDeque<(Instant, u64)>,
    debut: Instant,
    cumul: u64,
    /// Octets par seconde écoulée, depuis le début. Sert à repérer une
    /// tendance (cache épuisé, throttling thermique).
    profil: Vec<u64>,
    seconde_courante: u64,
    octets_seconde: u64,
    blocages: u32,
    plus_longue_pause: Duration,
    dernier_octet: Instant,
}

impl Default for Debitmetre {
    fn default() -> Self {
        Self::new()
    }
}

impl Debitmetre {
    pub fn new() -> Self {
        let maintenant = Instant::now();
        Debitmetre {
            echantillons: VecDeque::new(),
            debut: maintenant,
            cumul: 0,
            profil: Vec::new(),
            seconde_courante: 0,
            octets_seconde: 0,
            blocages: 0,
            plus_longue_pause: Duration::ZERO,
            dernier_octet: maintenant,
        }
    }

    pub fn ajouter(&mut self, octets: u64) {
        let maintenant = Instant::now();

        let pause = maintenant.saturating_duration_since(self.dernier_octet);
        if pause >= SEUIL_BLOCAGE {
            self.blocages += 1;
            if pause > self.plus_longue_pause {
                self.plus_longue_pause = pause;
            }
        }
        self.dernier_octet = maintenant;

        self.cumul += octets;
        self.echantillons.push_back((maintenant, octets));
        while let Some((t, o)) = self.echantillons.front().copied() {
            if maintenant.duration_since(t) > FENETRE {
                self.echantillons.pop_front();
                let _ = o;
            } else {
                break;
            }
        }

        // Profil seconde par seconde.
        let seconde = maintenant.duration_since(self.debut).as_secs();
        if seconde != self.seconde_courante {
            while self.profil.len() as u64 <= self.seconde_courante {
                self.profil.push(0);
            }
            if let Some(c) = self.profil.get_mut(self.seconde_courante as usize) {
                *c = self.octets_seconde;
            }
            self.seconde_courante = seconde;
            self.octets_seconde = 0;
        }
        self.octets_seconde += octets;
    }

    /// Débit sur la fenêtre glissante, en octets par seconde.
    pub fn debit(&self) -> f64 {
        let Some((premier, _)) = self.echantillons.front().copied() else {
            return 0.0;
        };
        let duree = Instant::now().duration_since(premier).as_secs_f64();
        if duree < 0.2 {
            return 0.0;
        }
        let total: u64 = self.echantillons.iter().map(|(_, o)| *o).sum();
        total as f64 / duree
    }

    pub fn cumul(&self) -> u64 {
        self.cumul
    }

    pub fn debit_moyen(&self) -> f64 {
        let duree = self.dernier_octet.duration_since(self.debut).as_secs_f64();
        if duree <= 0.0 {
            0.0
        } else {
            self.cumul as f64 / duree
        }
    }

    pub fn bilan(&self) -> ProfilDebit {
        let mut profil = self.profil.clone();
        while profil.len() as u64 <= self.seconde_courante {
            profil.push(0);
        }
        if let Some(c) = profil.get_mut(self.seconde_courante as usize) {
            *c = self.octets_seconde;
        }
        ProfilDebit {
            octets: self.cumul,
            duree_s: self.dernier_octet.duration_since(self.debut).as_secs_f64(),
            debit_moyen: self.debit_moyen(),
            par_seconde: profil,
            blocages: self.blocages,
            plus_longue_pause_ms: self.plus_longue_pause.as_millis() as u64,
        }
    }
}

/// Trace du débit d'un flux, conservée dans le rapport.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ProfilDebit {
    pub octets: u64,
    pub duree_s: f64,
    pub debit_moyen: f64,
    /// Octets écrits pendant chaque seconde écoulée.
    pub par_seconde: Vec<u64>,
    /// Pauses d'au moins une seconde sans le moindre octet.
    pub blocages: u32,
    pub plus_longue_pause_ms: u64,
}

impl ProfilDebit {
    /// Débit moyen sur la première et la dernière portion du transfert.
    /// Un net décrochage est la signature d'un cache épuisé ou d'un
    /// throttling thermique.
    pub fn debut_et_fin(&self) -> Option<(f64, f64)> {
        // Il faut assez de matière pour que la comparaison ait un sens.
        if self.par_seconde.len() < 20 {
            return None;
        }
        let tiers = self.par_seconde.len() / 3;
        let debut: u64 = self.par_seconde[..tiers].iter().sum();
        let fin: u64 = self.par_seconde[self.par_seconde.len() - tiers..]
            .iter()
            .sum();
        Some((debut as f64 / tiers as f64, fin as f64 / tiers as f64))
    }

    /// Écart-type rapporté à la moyenne, sur les secondes actives.
    pub fn irregularite(&self) -> Option<f64> {
        let actives: Vec<f64> = self
            .par_seconde
            .iter()
            .filter(|o| **o > 0)
            .map(|o| *o as f64)
            .collect();
        if actives.len() < 10 {
            return None;
        }
        let moyenne = actives.iter().sum::<f64>() / actives.len() as f64;
        if moyenne <= 0.0 {
            return None;
        }
        let variance =
            actives.iter().map(|v| (v - moyenne).powi(2)).sum::<f64>() / actives.len() as f64;
        Some(variance.sqrt() / moyenne)
    }
}

/// Chronomètre cumulatif : sert à imputer le temps d'attente.
#[derive(Default)]
pub struct Cumul {
    total: Duration,
}

impl Cumul {
    pub fn mesurer<T>(&mut self, f: impl FnOnce() -> T) -> T {
        let t0 = Instant::now();
        let r = f();
        self.total += t0.elapsed();
        r
    }

    pub fn ajouter(&mut self, d: Duration) {
        self.total += d;
    }

    pub fn ms(&self) -> u64 {
        self.total.as_millis() as u64
    }

    pub fn duree(&self) -> Duration {
        self.total
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn le_profil_repere_un_decrochage() {
        let mut p = ProfilDebit {
            par_seconde: vec![100; 30],
            ..Default::default()
        };
        // Dernier tiers deux fois plus lent.
        for v in p.par_seconde.iter_mut().skip(20) {
            *v = 50;
        }
        let (debut, fin) = p.debut_et_fin().unwrap();
        assert_eq!(debut, 100.0);
        assert_eq!(fin, 50.0);
    }

    #[test]
    fn un_debit_regulier_nest_pas_irregulier() {
        let p = ProfilDebit {
            par_seconde: vec![100; 30],
            ..Default::default()
        };
        assert!(p.irregularite().unwrap() < 0.01);
    }

    #[test]
    fn un_debit_en_dents_de_scie_est_irregulier() {
        let mut par_seconde = Vec::new();
        for i in 0..30 {
            par_seconde.push(if i % 2 == 0 { 200 } else { 20 });
        }
        let p = ProfilDebit {
            par_seconde,
            ..Default::default()
        };
        assert!(p.irregularite().unwrap() > 0.5);
    }

    #[test]
    fn trop_peu_de_donnees_pour_conclure() {
        let p = ProfilDebit {
            par_seconde: vec![100; 5],
            ..Default::default()
        };
        assert!(
            p.debut_et_fin().is_none(),
            "ne doit rien conclure sur 5 secondes"
        );
        assert!(p.irregularite().is_none());
    }
}
