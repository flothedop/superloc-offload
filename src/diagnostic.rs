//! Diagnostic de lenteur.
//!
//! Règle de conduite : ce module ne prononce jamais une cause. Il sépare
//! strictement ce qui a été **mesuré** de ce qui est **compatible avec ces
//! mesures**, et chaque hypothèse est accompagnée du test qui permettrait
//! de la confirmer. Un outil dont la valeur est la fiabilité n'a pas le
//! droit de se tromper avec assurance.

use crate::mesure::ProfilDebit;
use crate::report::Rapport;
use crate::volume::Volume;
use serde::{Deserialize, Serialize};

/// En dessous, il vaut la peine de regarder pourquoi (octets par seconde).
const PLANCHER: f64 = 60.0 * 1_000_000.0;
/// Plage de débit compatible avec un lien USB 2.
const USB2_BAS: f64 = 26.0 * 1_000_000.0;
const USB2_HAUT: f64 = 46.0 * 1_000_000.0;
/// Décrochage jugé significatif entre le début et la fin du transfert.
const DECROCHAGE: f64 = 0.6;
/// Au-delà, le débit est jugé irrégulier.
const IRREGULARITE: f64 = 0.45;
/// Durée minimale pour qu'une tendance veuille dire quelque chose.
const SECONDES_MINIMUM: usize = 20;
/// Taille en dessous de laquelle un fichier coûte surtout des métadonnées.
const PETIT_FICHIER: u64 = 10 * 1_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Goulot {
    /// La carte ou son lecteur limitent la copie.
    Source,
    /// Cette destination limite la copie.
    Destination(usize),
    /// Rien ne se détache nettement.
    Indetermine,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Hypothese {
    pub titre: String,
    /// Ce qui, dans les mesures, rend cette hypothèse plausible.
    pub parce_que: String,
    /// Le geste concret qui permet de trancher.
    pub verifier: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Diagnostic {
    /// Vrai si le débit mérite qu'on s'y arrête.
    pub lent: bool,
    pub goulot: Option<Goulot>,
    /// Constats issus de la mesure. Ce sont des faits.
    pub faits: Vec<String>,
    /// Explications compatibles avec ces faits, de la plus à la moins
    /// probable. Ce ne sont pas des conclusions.
    pub hypotheses: Vec<Hypothese>,
}

impl Diagnostic {
    pub fn a_quelque_chose_a_dire(&self) -> bool {
        !self.faits.is_empty() || !self.hypotheses.is_empty()
    }
}

pub struct Flux<'a> {
    pub nom: String,
    pub profil: &'a ProfilDebit,
    pub volume: Option<&'a Volume>,
    /// Temps pendant lequel la lecture de la carte a attendu ce disque.
    pub attente_lecture_ms: u64,
    pub temps_verif_ms: u64,
}

pub struct Entree<'a> {
    pub source: Flux<'a>,
    pub destinations: Vec<Flux<'a>>,
    pub nb_fichiers: usize,
    pub taille_mediane: u64,
    pub octets_total: u64,
}

pub fn debit_lisible(octets_par_s: f64) -> String {
    if octets_par_s <= 0.0 {
        return "—".to_string();
    }
    let mo = octets_par_s / 1_000_000.0;
    if mo >= 1000.0 {
        format!("{:.2} Go/s", mo / 1000.0)
    } else if mo >= 100.0 {
        format!("{mo:.0} Mo/s")
    } else {
        format!("{mo:.1} Mo/s")
    }
}

fn duree_lisible(ms: u64) -> String {
    let s = ms as f64 / 1000.0;
    if s >= 60.0 {
        format!("{:.0} min", s / 60.0)
    } else {
        format!("{s:.0} s")
    }
}

/// Désigne le maillon limitant, à partir du temps d'attente imputé.
fn goulot(entree: &Entree) -> Option<Goulot> {
    let attentes: Vec<u64> = entree
        .destinations
        .iter()
        .map(|d| d.attente_lecture_ms)
        .collect();
    let total_attente: u64 = attentes.iter().sum();
    let duree_totale_ms = (entree.source.profil.duree_s * 1000.0) as u64;
    if duree_totale_ms < 5_000 {
        return None;
    }

    // Une destination qui a fait attendre la lecture plus de 40 % du temps
    // total est le maillon limitant.
    if let Some((i, plus_lente)) = attentes.iter().enumerate().max_by_key(|(_, a)| **a) {
        if *plus_lente * 100 / duree_totale_ms.max(1) >= 40 {
            return Some(Goulot::Destination(i));
        }
    }
    // Sinon, si les destinations n'ont presque jamais fait attendre, c'est
    // la carte qui donne le rythme.
    if total_attente * 100 / duree_totale_ms.max(1) < 10 {
        return Some(Goulot::Source);
    }
    Some(Goulot::Indetermine)
}

/// Analyse les mesures et produit un diagnostic.
pub fn analyser(entree: &Entree) -> Diagnostic {
    let mut d = Diagnostic {
        goulot: goulot(entree),
        ..Default::default()
    };

    let le_plus_lent = entree.destinations.iter().min_by(|a, b| {
        a.profil
            .debit_moyen
            .partial_cmp(&b.profil.debit_moyen)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let Some(lent) = le_plus_lent else {
        return d;
    };
    let profil = lent.profil;

    // Trop tôt pour dire quoi que ce soit.
    if profil.par_seconde.len() < SECONDES_MINIMUM {
        return d;
    }

    d.lent = profil.debit_moyen > 0.0 && profil.debit_moyen < PLANCHER;

    // ---------------------------------------------------------------- Faits
    match d.goulot {
        Some(Goulot::Destination(i)) => {
            if let Some(dest) = entree.destinations.get(i) {
                let part = dest.attente_lecture_ms as f64
                    / (entree.source.profil.duree_s * 1000.0).max(1.0)
                    * 100.0;
                d.faits.push(format!(
                    "La copie a attendu {} pendant {:.0} % du temps ({}). C'est ce disque qui donne le rythme.",
                    dest.nom,
                    part,
                    duree_lisible(dest.attente_lecture_ms)
                ));
            }
        }
        Some(Goulot::Source) => d.faits.push(format!(
            "Aucune destination n'a fait attendre la copie : la carte lit à {} et c'est elle qui limite.",
            debit_lisible(entree.source.profil.debit_moyen)
        )),
        _ => {}
    }

    for dest in &entree.destinations {
        d.faits.push(format!(
            "{} : {} en moyenne à l'écriture.",
            dest.nom,
            debit_lisible(dest.profil.debit_moyen)
        ));
    }

    if profil.blocages > 0 {
        d.faits.push(format!(
            "{} : {} pause{} d'au moins une seconde, la plus longue de {}.",
            lent.nom,
            profil.blocages,
            if profil.blocages > 1 { "s" } else { "" },
            duree_lisible(profil.plus_longue_pause_ms)
        ));
    }

    let beaucoup_de_petits = entree.taille_mediane < PETIT_FICHIER && entree.nb_fichiers > 100;
    if beaucoup_de_petits {
        d.faits.push(format!(
            "{} fichiers dont la moitié font moins de {} : le débit chute par arithmétique, chaque fichier coûte une ouverture, une écriture physique et une relecture.",
            entree.nb_fichiers,
            octets_lisibles(entree.taille_mediane)
        ));
    }

    if let Some(v) = lent.volume {
        if v.reseau {
            d.faits.push(format!(
                "{} est un volume réseau ({}) : le réseau, et non le disque, fixe le plafond.",
                lent.nom, v.systeme_fichiers
            ));
        } else {
            d.faits.push(format!(
                "{} est formaté en {}.",
                lent.nom, v.systeme_fichiers
            ));
        }
    }

    if lent.temps_verif_ms > 0 {
        d.faits.push(format!(
            "Vérification comprise : chaque octet est écrit puis relu depuis le support ({} de relecture sur {}).",
            duree_lisible(lent.temps_verif_ms),
            lent.nom
        ));
    }

    // ----------------------------------------------------------- Hypothèses
    let irregulier = profil
        .irregularite()
        .map(|i| i > IRREGULARITE)
        .unwrap_or(false);

    // Plateau compatible avec un lien USB 2 : régulier, dans la bonne plage.
    if profil.debit_moyen > USB2_BAS && profil.debit_moyen < USB2_HAUT && !irregulier {
        d.hypotheses.push(Hypothese {
            titre: "Le disque est peut-être branché sur un port USB 2".into(),
            parce_que: format!(
                "le débit plafonne à {} et reste très régulier, ce qui correspond au plafond pratique de l'USB 2",
                debit_lisible(profil.debit_moyen)
            ),
            verifier: "Rebrancher sur un autre port, de préférence marqué SS ou bleu, et comparer le débit affiché.".into(),
        });
    }

    // Décrochage franc et durable.
    if let Some((debut, fin)) = profil.debut_et_fin() {
        if debut > 0.0 && fin < debut * DECROCHAGE {
            d.hypotheses.push(Hypothese {
                titre: "Le disque a peut-être épuisé son cache rapide, ou il chauffe".into(),
                parce_que: format!(
                    "le débit est passé de {} au début à {} sur la fin, sans remonter",
                    debit_lisible(debut),
                    debit_lisible(fin)
                ),
                verifier: "Laisser le disque refroidir quelques minutes, puis relancer une copie et regarder si le débit repart haut avant de retomber au même endroit.".into(),
            });
        }
    }

    // Débit heurté : écriture interrompue par un tiers.
    if irregulier || profil.blocages >= 3 {
        d.hypotheses.push(Hypothese {
            titre: "Un autre programme lit ou analyse les fichiers pendant l'écriture".into(),
            parce_que: "le débit est en dents de scie plutôt que bas mais régulier, ce qui ressemble à une interruption extérieure — antivirus en temps réel, indexation, sauvegarde automatique".into(),
            verifier: "Exclure le dossier de destination de l'analyse en temps réel de l'antivirus et de l'indexation, puis relancer.".into(),
        });
        d.hypotheses.push(Hypothese {
            titre: "Le disque lui-même peine".into(),
            parce_que: "des pauses franches pendant l'écriture peuvent aussi venir du support — secteurs en difficulté, disque mécanique qui se repositionne, alimentation insuffisante sur un port sans alimentation propre".into(),
            verifier: "Essayer le même déchargement vers un autre disque : si les pauses disparaissent, le support est en cause.".into(),
        });
    }

    // exFAT et petits fichiers.
    if beaucoup_de_petits {
        if let Some(v) = lent.volume {
            if v.couteux_en_petits_fichiers() {
                d.hypotheses.push(Hypothese {
                    titre: format!("Le format {} coûte cher en petits fichiers", v.systeme_fichiers),
                    parce_que: "ce système de fichiers écrit beaucoup de métadonnées par fichier, ce qui se voit surtout quand les fichiers sont nombreux et petits".into(),
                    verifier: "Comparer le débit sur un seul gros fichier : s'il remonte franchement, c'est le nombre de fichiers et non le disque.".into(),
                });
            }
        }
    }

    // Câble ou connectique, en dernier recours, seulement si c'est bas.
    if d.lent && d.hypotheses.is_empty() {
        d.hypotheses.push(Hypothese {
            titre: "Le lien physique est peut-être en cause".into(),
            parce_que: format!(
                "le débit est bas ({}) sans irrégularité ni décrochage qui orienterait vers une autre explication",
                debit_lisible(profil.debit_moyen)
            ),
            verifier: "Changer de câble, puis de port, en testant un changement à la fois.".into(),
        });
    }

    d
}

fn octets_lisibles(n: u64) -> String {
    crate::preflight::octets_lisibles(n)
}

/// Construit le diagnostic à partir d'un rapport terminé.
pub fn depuis_rapport(rapport: &Rapport) -> Diagnostic {
    let mut tailles: Vec<u64> = rapport.fichiers.iter().map(|f| f.taille).collect();
    tailles.sort_unstable();
    let mediane = tailles.get(tailles.len() / 2).copied().unwrap_or(0);

    let destinations: Vec<Flux> = rapport
        .destinations
        .iter()
        .map(|d| Flux {
            nom: nom_court(&d.dossier),
            profil: &d.profil,
            volume: d.volume.as_ref(),
            attente_lecture_ms: d.attente_lecture_ms,
            temps_verif_ms: d.temps_verif_ms,
        })
        .collect();

    analyser(&Entree {
        source: Flux {
            nom: nom_court(&rapport.source),
            profil: &rapport.profil_source,
            volume: rapport.volume_source.as_ref(),
            attente_lecture_ms: 0,
            temps_verif_ms: 0,
        },
        destinations,
        nb_fichiers: rapport.nb_fichiers,
        taille_mediane: mediane,
        octets_total: rapport.octets_total,
    })
}

fn nom_court(chemin: &str) -> String {
    chemin
        .replace('\\', "/")
        .rsplit('/')
        .find(|s| !s.is_empty())
        .unwrap_or(chemin)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profil(par_seconde: Vec<u64>, blocages: u32) -> ProfilDebit {
        let octets: u64 = par_seconde.iter().sum();
        let duree = par_seconde.len() as f64;
        ProfilDebit {
            octets,
            duree_s: duree,
            debit_moyen: octets as f64 / duree,
            par_seconde,
            blocages,
            plus_longue_pause_ms: if blocages > 0 { 2400 } else { 0 },
        }
    }

    fn entree<'a>(source: &'a ProfilDebit, dest: &'a ProfilDebit, attente_ms: u64) -> Entree<'a> {
        Entree {
            source: Flux {
                nom: "A001".into(),
                profil: source,
                volume: None,
                attente_lecture_ms: 0,
                temps_verif_ms: 0,
            },
            destinations: vec![Flux {
                nom: "BACKUP".into(),
                profil: dest,
                volume: None,
                attente_lecture_ms: attente_ms,
                temps_verif_ms: 1000,
            }],
            nb_fichiers: 40,
            taille_mediane: 8_000_000_000,
            octets_total: 320_000_000_000,
        }
    }

    #[test]
    fn ne_conclut_rien_sur_un_transfert_trop_court() {
        let p = profil(vec![30_000_000; 6], 0);
        let d = analyser(&entree(&p, &p, 0));
        assert!(!d.lent);
        assert!(d.hypotheses.is_empty(), "aucune hypothèse sur 6 secondes");
    }

    #[test]
    fn impute_le_goulot_a_la_destination_qui_fait_attendre() {
        let source = profil(vec![40_000_000; 60], 0);
        let dest = profil(vec![40_000_000; 60], 0);
        let d = analyser(&entree(&source, &dest, 45_000));
        assert_eq!(d.goulot, Some(Goulot::Destination(0)));
        assert!(d.faits.iter().any(|f| f.contains("BACKUP")));
    }

    #[test]
    fn impute_le_goulot_a_la_carte_quand_personne_nattend() {
        let source = profil(vec![40_000_000; 60], 0);
        let dest = profil(vec![40_000_000; 60], 0);
        let d = analyser(&entree(&source, &dest, 100));
        assert_eq!(d.goulot, Some(Goulot::Source));
    }

    #[test]
    fn un_plateau_regulier_evoque_lusb2_sans_laffirmer() {
        let p = profil(vec![36_000_000; 60], 0);
        let d = analyser(&entree(&p, &p, 30_000));
        let h = d
            .hypotheses
            .iter()
            .find(|h| h.titre.contains("USB 2"))
            .expect("hypothèse USB 2 attendue");
        assert!(h.titre.contains("peut-être"), "doit rester une hypothèse");
        assert!(!h.verifier.is_empty(), "doit proposer un test");
    }

    #[test]
    fn un_debit_rapide_et_regulier_nevoque_rien() {
        let p = profil(vec![400_000_000; 60], 0);
        let d = analyser(&entree(&p, &p, 1000));
        assert!(!d.lent);
        assert!(
            d.hypotheses.is_empty(),
            "aucune hypothèse ne doit être émise à 400 Mo/s : {:?}",
            d.hypotheses
        );
    }

    #[test]
    fn un_decrochage_durable_est_signale() {
        let mut par_seconde = vec![500_000_000u64; 20];
        par_seconde.extend(vec![80_000_000u64; 40]);
        let p = profil(par_seconde, 0);
        let d = analyser(&entree(&p, &p, 30_000));
        assert!(d
            .hypotheses
            .iter()
            .any(|h| h.titre.contains("cache rapide") || h.titre.contains("chauffe")));
    }

    #[test]
    fn un_debit_en_dents_de_scie_evoque_une_interruption_exterieure() {
        let par_seconde: Vec<u64> = (0..60)
            .map(|i| if i % 2 == 0 { 300_000_000 } else { 20_000_000 })
            .collect();
        let p = profil(par_seconde, 4);
        let d = analyser(&entree(&p, &p, 30_000));
        assert!(d
            .hypotheses
            .iter()
            .any(|h| h.titre.contains("autre programme")));
        assert!(d
            .hypotheses
            .iter()
            .any(|h| h.titre.contains("disque lui-même")));
    }

    #[test]
    fn chaque_hypothese_porte_un_test_concret() {
        let p = profil(vec![36_000_000; 60], 3);
        let d = analyser(&entree(&p, &p, 30_000));
        assert!(!d.hypotheses.is_empty());
        for h in &d.hypotheses {
            assert!(
                !h.parce_que.is_empty(),
                "« {} » sans justification",
                h.titre
            );
            assert!(!h.verifier.is_empty(), "« {} » sans test", h.titre);
        }
    }
}
