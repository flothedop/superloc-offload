//! Structures du rapport de déchargement.
//!
//! Ce rapport est la trace remise au client : il sert de source au PDF, au
//! journal, et récapitule le résultat destination par destination.

use crate::hash::Algo;
use crate::mesure::ProfilDebit;
use crate::uncached::Mode;
use crate::volume::Volume;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Etat {
    Ok,
    Echec,
}

impl Etat {
    pub fn ok(&self) -> bool {
        *self == Etat::Ok
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Empreinte {
    pub algo: Algo,
    pub valeur: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VerifDestination {
    /// Index de la destination dans [`Rapport::destinations`].
    pub destination: usize,
    pub etat: Etat,
    pub mode_lecture: Option<Mode>,
    pub erreur: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RapportFichier {
    pub chemin: String,
    pub clip: String,
    pub taille: u64,
    pub modifie_le: Option<String>,
    pub empreintes: Vec<Empreinte>,
    pub verifications: Vec<VerifDestination>,
}

impl RapportFichier {
    pub fn empreinte(&self, algo: Algo) -> Option<&str> {
        self.empreintes
            .iter()
            .find(|e| e.algo == algo)
            .map(|e| e.valeur.as_str())
    }

    pub fn ok_partout(&self) -> bool {
        !self.verifications.is_empty() && self.verifications.iter().all(|v| v.etat.ok())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RapportDestination {
    pub dossier: String,
    pub etat: Etat,
    pub erreur: Option<String>,
    pub fichiers_ok: usize,
    pub fichiers_echec: usize,
    pub octets_ecrits: u64,
    pub mhl: Option<String>,
    /// Caractéristiques du volume : système de fichiers, amovible, réseau.
    pub volume: Option<Volume>,
    /// Débit d'écriture mesuré sur le temps réel.
    pub profil: ProfilDebit,
    pub temps_ecriture_ms: u64,
    pub temps_sync_ms: u64,
    pub temps_verif_ms: u64,
    /// Temps pendant lequel la lecture de la carte a dû attendre cette
    /// destination. C'est cette mesure qui désigne le goulot d'étranglement.
    pub attente_lecture_ms: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Rapport {
    pub outil: String,
    pub nom: String,
    pub source: String,
    pub debut: String,
    pub fin: String,
    pub algos: Vec<Algo>,
    pub octets_total: u64,
    pub nb_fichiers: usize,
    pub nb_clips: usize,
    pub ignores: Vec<String>,
    pub liens: Vec<String>,
    pub fichiers: Vec<RapportFichier>,
    pub destinations: Vec<RapportDestination>,
    /// Relecture de la carte source après copie, si demandée.
    pub source_reverifiee: Option<Etat>,
    pub etat_global: Etat,
    pub avertissements: Vec<String>,
    pub machine: String,
    pub utilisateur: String,
    pub volume_source: Option<Volume>,
    pub profil_source: ProfilDebit,
    pub temps_lecture_ms: u64,
}

impl Rapport {
    /// Le voyant ne passe au vert que si chaque fichier est vérifié sur
    /// chaque destination, sans aucune destination en échec.
    pub fn recalculer_etat(&mut self) {
        let toutes_destinations_ok =
            !self.destinations.is_empty() && self.destinations.iter().all(|d| d.etat.ok());
        let tous_fichiers_ok = self.fichiers.iter().all(|f| f.ok_partout());
        let source_ok = self.source_reverifiee.map(|e| e.ok()).unwrap_or(true);

        self.etat_global = if toutes_destinations_ok && tous_fichiers_ok && source_ok {
            Etat::Ok
        } else {
            Etat::Echec
        };
    }

    pub fn ecrire_json(&self, chemin: &std::path::Path) -> std::io::Result<()> {
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(chemin, json)
    }
}

pub const OUTIL: &str = concat!("SuperLoc Offload ", env!("CARGO_PKG_VERSION"));
