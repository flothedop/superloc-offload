//! Rapport d'un déchargement et verdict.

use crate::diagnostic::Diagnostic;
use crate::erreur::Erreur;
use crate::es::{Durabilite, Residence};
use crate::hash::{Algo, Empreinte};
use crate::langue::Langue;
use crate::mesure::ProfilDebit;
use crate::sanscache::Mode;
use crate::volume::Volume;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Niveau de preuve obtenu pour une copie.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Niveau {
    /// Copie absente, illisible ou différente.
    Echec,
    /// Mode rapide : seule la taille a été comparée.
    Taille,
    /// Empreinte identique, sans preuve que la relecture venait du disque.
    CachePossible,
    /// Relue depuis le disque, empreinte identique.
    Support,
}

impl Niveau {
    pub fn libelle(self, l: Langue) -> &'static str {
        match self {
            Niveau::Support => l.t("relue depuis le disque", "read back from disk"),
            Niveau::CachePossible => l.t(
                "identique, relecture peut-être servie par le cache",
                "identical, read-back may have come from cache",
            ),
            Niveau::Taille => l.t("taille seule", "size only"),
            Niveau::Echec => l.t("échec", "failed"),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ModeVerif {
    /// Écriture, écriture physique, relecture de chaque copie.
    #[default]
    Complet,
    /// Idem, puis relecture de la carte.
    CompletCarte,
    /// Taille seule : rapide, jamais « bon à formater ».
    Taille,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Copie {
    pub destination: usize,
    pub niveau: Niveau,
    pub erreur: Option<Erreur>,
    pub durabilite: Option<Durabilite>,
    pub lecture: Option<Mode>,
    pub residence: Option<Residence>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FichierRapport {
    pub chemin: String,
    pub clip: String,
    /// Octets réellement lus sur la carte.
    pub taille: u64,
    pub taille_annoncee: u64,
    pub modifie: Option<DateTime<Utc>>,
    pub cree: Option<DateTime<Utc>>,
    /// Empreintes calculées à la lecture de la carte.
    pub empreintes: Vec<Empreinte>,
    pub erreur_source: Option<Erreur>,
    pub copies: Vec<Copie>,
    /// Relecture finale de la carte : `Some(true)` identique.
    pub relecture_carte: Option<bool>,
}

impl FichierRapport {
    pub fn empreinte(&self, algo: Algo) -> Option<&str> {
        self.empreintes
            .iter()
            .find(|e| e.algo == algo)
            .map(|e| e.valeur.as_str())
    }

    pub fn copie(&self, destination: usize) -> Option<&Copie> {
        self.copies.iter().find(|c| c.destination == destination)
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct DestinationRapport {
    /// Dossier du déchargement sur cette destination.
    pub dossier: String,
    pub volume: Option<Volume>,
    pub erreur_fatale: Option<Erreur>,
    pub fichiers_ok: usize,
    pub fichiers_echec: usize,
    pub octets_ecrits: u64,
    pub profil: ProfilDebit,
    pub temps_ecriture_ms: u64,
    pub temps_sync_ms: u64,
    pub temps_verif_ms: u64,
    /// Temps pendant lequel la lecture de la carte a attendu ce disque.
    pub attente_imputee_ms: u64,
    /// Manifestes écrits, chemins relatifs au dossier.
    pub manifestes: Vec<String>,
    pub durabilite_degradee: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Verdict {
    BonAFormater,
    ConformeNePasFormater,
    Echec,
    Annule,
}

impl Verdict {
    pub fn titre(self, l: Langue) -> &'static str {
        match self {
            Verdict::BonAFormater => l.t("BON À FORMATER", "SAFE TO FORMAT"),
            Verdict::ConformeNePasFormater => l.t(
                "COPIE CONFORME — NE PAS FORMATER ENCORE",
                "COPY VERIFIED — DO NOT FORMAT YET",
            ),
            Verdict::Echec => l.t("ÉCHEC — NE PAS FORMATER", "FAILED — DO NOT FORMAT"),
            Verdict::Annule => l.t("ANNULÉ — NE PAS FORMATER", "CANCELLED — DO NOT FORMAT"),
        }
    }
}

/// Ce qui empêche le verdict « bon à formater », ou mérite d'être su.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "code", rename_all = "kebab-case")]
pub enum Remarque {
    UneSeuleCopie,
    MemeDisquePhysique { destinations: Vec<usize> },
    DisquesNonIdentifies,
    CachePossible { destination: usize, fichiers: usize },
    ModeTaille,
    FichiersNonCopies { nombre: usize },
    RelectureCarteDifferente { fichiers: usize },
    DestinationEnEchec { destination: usize },
    DurabiliteDegradee { destination: usize },
    SegmentsManquants { segments: Vec<String> },
    LiensIgnores { nombre: usize },
    TaillesAnnoncees { fichiers: usize },
    VolumeReseau { destination: usize },
    Annulation,
}

impl Remarque {
    pub fn texte(&self, l: Langue, noms: &[String]) -> String {
        let nom = |i: &usize| {
            noms.get(*i)
                .cloned()
                .unwrap_or_else(|| format!("#{}", i + 1))
        };
        match self {
            Remarque::UneSeuleCopie => l
                .t(
                    "Une seule copie vérifiée : il en faut deux, sur deux disques différents, avant de formater la carte.",
                    "Only one verified copy: two are needed, on two different disks, before formatting the card.",
                )
                .into(),
            Remarque::MemeDisquePhysique { destinations } => {
                let n: Vec<String> = destinations.iter().map(nom).collect();
                match l {
                    Langue::Fr => format!("{} sont sur le même disque physique : cela ne compte que pour une copie.", n.join(" et ")),
                    Langue::En => format!("{} are on the same physical disk: this counts as one copy only.", n.join(" and ")),
                }
            }
            Remarque::DisquesNonIdentifies => l
                .t(
                    "Le système n'a pas permis d'identifier les disques physiques : vérifiez vous-même que les copies sont sur deux disques différents.",
                    "The system could not identify the physical disks: check yourself that the copies are on two different disks.",
                )
                .into(),
            Remarque::CachePossible { destination, fichiers } => match l {
                Langue::Fr => format!("{} : {fichiers} fichier(s) identique(s), mais le système n'a pas permis de prouver que la relecture venait du disque.", nom(destination)),
                Langue::En => format!("{}: {fichiers} file(s) identical, but the system could not prove the read-back came from the disk.", nom(destination)),
            },
            Remarque::ModeTaille => l
                .t(
                    "Mode « taille seule » : les copies n'ont pas été relues.",
                    "“Size only” mode: the copies were not read back.",
                )
                .into(),
            Remarque::FichiersNonCopies { nombre } => match l {
                Langue::Fr => format!("{nombre} entrée(s) de la carte n'ont pas pu être lues ni copiées."),
                Langue::En => format!("{nombre} card entr(ies) could not be read or copied."),
            },
            Remarque::RelectureCarteDifferente { fichiers } => match l {
                Langue::Fr => format!("{fichiers} fichier(s) relu(s) différemment sur la carte : lecteur de cartes ou carte suspect."),
                Langue::En => format!("{fichiers} file(s) read differently from the card: card reader or card suspect."),
            },
            Remarque::DestinationEnEchec { destination } => match l {
                Langue::Fr => format!("{} : copie en échec.", nom(destination)),
                Langue::En => format!("{}: copy failed.", nom(destination)),
            },
            Remarque::DurabiliteDegradee { destination } => match l {
                Langue::Fr => format!("{} : le système de fichiers n'a pas confirmé le vidage du cache du disque. Éjectez-le proprement avant de le débrancher.", nom(destination)),
                Langue::En => format!("{}: the file system did not confirm the disk cache flush. Eject it properly before unplugging.", nom(destination)),
            },
            Remarque::SegmentsManquants { segments } => match l {
                Langue::Fr => format!("Numérotation de segments interrompue sur la carte (déjà avant copie) : {}.", segments.join(", ")),
                Langue::En => format!("Segment numbering has a gap on the card (before copy): {}.", segments.join(", ")),
            },
            Remarque::LiensIgnores { nombre } => match l {
                Langue::Fr => format!("{nombre} lien(s) symbolique(s) ignoré(s)."),
                Langue::En => format!("{nombre} symbolic link(s) skipped."),
            },
            Remarque::TaillesAnnoncees { fichiers } => match l {
                Langue::Fr => format!("{fichiers} fichier(s) dont la taille annoncée par le système différait de la taille lue (normal sur un volume Codex HDE)."),
                Langue::En => format!("{fichiers} file(s) whose size reported by the system differed from the size read (normal on a Codex HDE volume)."),
            },
            Remarque::VolumeReseau { destination } => match l {
                Langue::Fr => format!("{} est un volume réseau : la relecture peut être servie par le serveur.", nom(destination)),
                Langue::En => format!("{} is a network volume: the read-back may be served by the server.", nom(destination)),
            },
            Remarque::Annulation => l.t("Copie annulée.", "Copy cancelled.").into(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Rapport {
    pub outil: String,
    pub version: String,
    pub nom: String,
    pub source: String,
    pub volume_source: Option<Volume>,
    pub debut: DateTime<Utc>,
    pub fin: DateTime<Utc>,
    pub mode: ModeVerif,
    pub algos: Vec<Algo>,
    pub algo_comparaison: Algo,
    pub operateur: String,
    pub projet: String,
    pub machine: String,
    pub utilisateur: String,
    pub nb_fichiers: usize,
    pub octets: u64,
    pub nb_clips: usize,
    pub dossiers: usize,
    pub dossiers_vides: usize,
    pub exclus: Vec<String>,
    pub liens: Vec<String>,
    pub illisibles: Vec<Erreur>,
    pub fichiers: Vec<FichierRapport>,
    pub destinations: Vec<DestinationRapport>,
    pub annule: bool,
    pub verdict: Verdict,
    pub remarques: Vec<Remarque>,
    pub profil_source: ProfilDebit,
    pub temps_lecture_ms: u64,
    pub diagnostic: Option<Diagnostic>,
}

impl Rapport {
    pub fn noms_destinations(&self) -> Vec<String> {
        self.destinations
            .iter()
            .map(|d| match &d.volume {
                Some(v) if !v.nom.is_empty() => v.nom.clone(),
                _ => d.dossier.clone(),
            })
            .collect()
    }

    pub fn duree_s(&self) -> f64 {
        (self.fin - self.debut).num_milliseconds().max(0) as f64 / 1000.0
    }

    /// Niveau le plus faible des copies de cette destination.
    pub fn niveau_destination(&self, d: usize) -> Niveau {
        if self
            .destinations
            .get(d)
            .and_then(|x| x.erreur_fatale.as_ref())
            .is_some()
        {
            return Niveau::Echec;
        }
        self.fichiers
            .iter()
            .map(|f| f.copie(d).map(|c| c.niveau).unwrap_or(Niveau::Echec))
            .min()
            .unwrap_or(Niveau::Support)
    }

    /// Calcule le verdict et les remarques. Règle : « bon à formater »
    /// exige au moins deux destinations dont TOUS les fichiers sont relus
    /// depuis le disque, sur deux disques physiques différents.
    pub fn conclure(&mut self, segments_manquants: Vec<String>) {
        let mut r = Vec::new();
        let n = self.destinations.len();
        let niveaux: Vec<Niveau> = (0..n).map(|d| self.niveau_destination(d)).collect();

        if self.annule {
            r.push(Remarque::Annulation);
        }
        if self.mode == ModeVerif::Taille {
            r.push(Remarque::ModeTaille);
        }
        if !self.illisibles.is_empty() {
            r.push(Remarque::FichiersNonCopies {
                nombre: self.illisibles.len(),
            });
        }
        let differentes = self
            .fichiers
            .iter()
            .filter(|f| f.relecture_carte == Some(false))
            .count();
        if differentes > 0 {
            r.push(Remarque::RelectureCarteDifferente {
                fichiers: differentes,
            });
        }
        for (d, niveau) in niveaux.iter().enumerate() {
            if *niveau == Niveau::Echec {
                r.push(Remarque::DestinationEnEchec { destination: d });
            }
            let cache = self
                .fichiers
                .iter()
                .filter(|f| f.copie(d).map(|c| c.niveau) == Some(Niveau::CachePossible))
                .count();
            if cache > 0 && *niveau == Niveau::CachePossible {
                r.push(Remarque::CachePossible {
                    destination: d,
                    fichiers: cache,
                });
            }
            if self.destinations[d].durabilite_degradee {
                r.push(Remarque::DurabiliteDegradee { destination: d });
            }
            if self.destinations[d]
                .volume
                .as_ref()
                .map(|v| v.reseau)
                .unwrap_or(false)
            {
                r.push(Remarque::VolumeReseau { destination: d });
            }
        }

        // Destinations entièrement relues depuis le disque, regroupées par
        // disque physique.
        let bonnes: Vec<usize> = (0..n).filter(|d| niveaux[*d] == Niveau::Support).collect();
        let mut groupes: Vec<(String, Vec<usize>)> = Vec::new();
        let mut non_identifie = false;
        for d in &bonnes {
            let v = self.destinations[*d].volume.as_ref();
            let cle = match (
                v.and_then(|v| v.id_disque.clone()),
                v.and_then(|v| v.id_volume.clone()),
            ) {
                (Some(disque), _) => format!("disque:{disque}"),
                (None, Some(vol)) => {
                    non_identifie = true;
                    format!("volume:{vol}")
                }
                (None, None) => {
                    non_identifie = true;
                    format!("destination:{d}")
                }
            };
            match groupes.iter_mut().find(|(c, _)| *c == cle) {
                Some((_, membres)) => membres.push(*d),
                None => groupes.push((cle, vec![*d])),
            }
        }
        for (_, membres) in &groupes {
            if membres.len() > 1 {
                r.push(Remarque::MemeDisquePhysique {
                    destinations: membres.clone(),
                });
            }
        }
        let disques_distincts = groupes.len();
        if disques_distincts < 2 && !self.annule && self.mode != ModeVerif::Taille {
            r.push(Remarque::UneSeuleCopie);
        }
        if disques_distincts >= 2 && non_identifie {
            r.push(Remarque::DisquesNonIdentifies);
        }

        if !segments_manquants.is_empty() {
            r.push(Remarque::SegmentsManquants {
                segments: segments_manquants,
            });
        }
        if !self.liens.is_empty() {
            r.push(Remarque::LiensIgnores {
                nombre: self.liens.len(),
            });
        }
        let tailles = self
            .fichiers
            .iter()
            .filter(|f| f.erreur_source.is_none() && f.taille != f.taille_annoncee)
            .count();
        if tailles > 0 {
            r.push(Remarque::TaillesAnnoncees { fichiers: tailles });
        }

        let aucun_echec = niveaux.iter().all(|n| *n > Niveau::Echec)
            && self.fichiers.iter().all(|f| f.erreur_source.is_none());

        // Seules comptent les destinations entièrement relues depuis le
        // disque : une troisième copie en échec ou non prouvée n'enlève rien
        // à deux bonnes copies sur deux disques distincts (elle est signalée).
        let bloquant = self.mode == ModeVerif::Taille
            || !self.illisibles.is_empty()
            || differentes > 0
            || disques_distincts < 2;

        self.verdict = if self.annule {
            Verdict::Annule
        } else if !bloquant {
            Verdict::BonAFormater
        } else if aucun_echec && self.illisibles.is_empty() && differentes == 0 && n > 0 {
            Verdict::ConformeNePasFormater
        } else {
            Verdict::Echec
        };
        self.remarques = r;
    }

    pub fn json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".into())
    }
}
