//! Injection de pannes, pour les tests.
//!
//! Corrompre une copie après coup ne prouve pas grand-chose : il faut que la
//! panne survienne PENDANT la copie, là où elle survient dans la vraie vie
//! (bit retourné par un câble, disque plein, disque débranché, secteur
//! illisible sur la carte). Le plan est vide en usage normal ; le moteur le
//! consulte aux points précis où ces pannes se produisent.

use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Panne {
    /// Retourne un bit des données écrites sur la destination, à cet octet
    /// du fichier dont le chemin relatif se termine par `fichier`.
    BitEcrit {
        destination: usize,
        fichier: String,
        octet: u64,
    },
    /// La destination déclare « disque plein » une fois `apres` octets
    /// écrits au total.
    DisquePlein { destination: usize, apres: u64 },
    /// La destination devient inaccessible en abordant ce fichier.
    Debranchement { destination: usize, fichier: String },
    /// N'écrit pas le dernier bloc du fichier, sans signaler d'erreur
    /// (écriture perdue par le système ou le disque).
    EcritureCourte { destination: usize, fichier: String },
    /// La lecture de ce fichier sur la carte échoue après `apres` octets.
    LectureSource { fichier: String, apres: u64 },
    /// La carte renvoie un octet différent à la relecture finale.
    SourceInstable { fichier: String },
    /// Ralentit chaque écriture de bloc sur cette destination.
    Lenteur {
        destination: usize,
        ms_par_bloc: u64,
    },
}

/// Fonction appelée après chaque bloc lu sur la carte (chemin relatif,
/// octets lus) : permet à un test de modifier la carte pendant la copie.
pub type Observateur = Arc<dyn Fn(&str, u64) + Send + Sync>;

#[derive(Clone, Default)]
pub struct Plan {
    pub pannes: Vec<Panne>,
    pub apres_bloc_source: Option<Observateur>,
}

impl std::fmt::Debug for Plan {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Plan")
            .field("pannes", &self.pannes)
            .finish()
    }
}

fn concerne(relatif: &str, fichier: &str) -> bool {
    !fichier.is_empty() && relatif.ends_with(fichier)
}

impl Plan {
    pub fn vide(&self) -> bool {
        self.pannes.is_empty() && self.apres_bloc_source.is_none()
    }

    /// Indice, dans le bloc qui commence à `debut` et fait `longueur`
    /// octets, du bit à retourner.
    pub fn bit_ecrit(&self, d: usize, relatif: &str, debut: u64, longueur: usize) -> Option<usize> {
        self.pannes.iter().find_map(|p| match p {
            Panne::BitEcrit {
                destination,
                fichier,
                octet,
            } if *destination == d
                && concerne(relatif, fichier)
                && *octet >= debut
                && *octet < debut + longueur as u64 =>
            {
                Some((*octet - debut) as usize)
            }
            _ => None,
        })
    }

    pub fn disque_plein(&self, d: usize, deja_ecrits: u64) -> bool {
        self.pannes.iter().any(|p| {
            matches!(p, Panne::DisquePlein { destination, apres } if *destination == d && deja_ecrits >= *apres)
        })
    }

    pub fn debranche(&self, d: usize, relatif: &str) -> bool {
        self.pannes.iter().any(|p| {
            matches!(p, Panne::Debranchement { destination, fichier } if *destination == d && concerne(relatif, fichier))
        })
    }

    pub fn ecriture_courte(&self, d: usize, relatif: &str) -> bool {
        self.pannes.iter().any(|p| {
            matches!(p, Panne::EcritureCourte { destination, fichier } if *destination == d && concerne(relatif, fichier))
        })
    }

    pub fn lecture_source(&self, relatif: &str, deja_lus: u64) -> bool {
        self.pannes.iter().any(|p| {
            matches!(p, Panne::LectureSource { fichier, apres } if concerne(relatif, fichier) && deja_lus >= *apres)
        })
    }

    pub fn source_instable(&self, relatif: &str) -> bool {
        self.pannes
            .iter()
            .any(|p| matches!(p, Panne::SourceInstable { fichier } if concerne(relatif, fichier)))
    }

    pub fn lenteur(&self, d: usize) -> Option<u64> {
        self.pannes.iter().find_map(|p| match p {
            Panne::Lenteur {
                destination,
                ms_par_bloc,
            } if *destination == d => Some(*ms_par_bloc),
            _ => None,
        })
    }
}
