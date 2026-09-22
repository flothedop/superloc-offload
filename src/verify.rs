//! Relecture et comparaison des empreintes.

use crate::hash::{Algo, MultiHasher};
use crate::report::Empreinte;
use crate::uncached::{self, Mode};
use crate::CHUNK;
use std::io;
use std::path::Path;

pub struct ResultatRelecture {
    pub mode: Mode,
    pub octets: u64,
    pub empreintes: Vec<(Algo, String)>,
}

/// Relit un fichier depuis le support, sans passer par le cache système,
/// et calcule ses empreintes.
pub fn relire(chemin: &Path, algos: &[Algo]) -> io::Result<ResultatRelecture> {
    let mut lecteur = uncached::ouvrir(chemin, CHUNK)?;
    let mut hasher = MultiHasher::new(algos);

    loop {
        let bloc = lecteur.lire_bloc()?;
        if bloc.is_empty() {
            break;
        }
        hasher.update(bloc);
    }

    Ok(ResultatRelecture {
        mode: lecteur.mode(),
        octets: hasher.bytes(),
        empreintes: hasher.finish(),
    })
}

/// Compare les empreintes relues à celles de la source.
///
/// Renvoie le détail de l'écart en cas de différence : c'est ce texte qui
/// sera affiché à l'utilisateur et consigné dans le rapport.
pub fn comparer(
    attendu: &[Empreinte],
    taille_attendue: u64,
    obtenu: &ResultatRelecture,
) -> Result<(), String> {
    if obtenu.octets != taille_attendue {
        return Err(format!(
            "taille différente : {} octets sur la destination contre {} à la source",
            obtenu.octets, taille_attendue
        ));
    }

    for e in attendu {
        match obtenu.empreintes.iter().find(|(a, _)| *a == e.algo) {
            None => return Err(format!("empreinte {} non recalculée", e.algo)),
            Some((_, valeur)) if *valeur != e.valeur => {
                return Err(format!(
                    "empreinte {} différente : {} à la destination contre {} à la source",
                    e.algo, valeur, e.valeur
                ));
            }
            Some(_) => {}
        }
    }

    if attendu.is_empty() {
        return Err("aucune empreinte de référence".to_string());
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Vérification d'un dossier déjà copié, à partir de son MHL.
// ---------------------------------------------------------------------------

use crate::report::Etat;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LigneVerif {
    pub fichier: String,
    pub etat: Etat,
    pub detail: Option<String>,
    pub mode_lecture: Option<Mode>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RapportVerif {
    pub dossier: String,
    pub mhls: Vec<String>,
    pub lignes: Vec<LigneVerif>,
    /// Fichiers listés au MHL mais absents du disque.
    pub absents: Vec<String>,
    /// Fichiers présents sur le disque mais absents du MHL.
    pub en_trop: Vec<String>,
    /// Copies interrompues restées sous leur nom temporaire.
    pub residus: Vec<String>,
    pub etat: Etat,
}

impl RapportVerif {
    pub fn nb_echecs(&self) -> usize {
        self.lignes.iter().filter(|l| !l.etat.ok()).count()
    }
}

/// Revérifie un dossier copié : chaque fichier listé au MHL est relu depuis
/// le support, sans cache, et son empreinte comparée. Signale également les
/// fichiers absents, les fichiers en trop et les copies interrompues.
pub fn controler_dossier(dossier: &Path) -> io::Result<RapportVerif> {
    let mhls = crate::mhl::trouver(dossier)?;
    if mhls.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!(
                "aucun fichier MHL dans {} : impossible de vérifier ce dossier",
                dossier.display()
            ),
        ));
    }

    let mut lignes = Vec::new();
    let mut absents = Vec::new();
    let mut connus: Vec<String> = Vec::new();

    for chemin_mhl in &mhls {
        for entree in crate::mhl::lire(chemin_mhl)? {
            connus.push(entree.fichier.clone());
            let cible = dossier.join(&entree.fichier);

            if !cible.is_file() {
                absents.push(entree.fichier.clone());
                lignes.push(LigneVerif {
                    fichier: entree.fichier,
                    etat: Etat::Echec,
                    detail: Some("fichier absent du disque".to_string()),
                    mode_lecture: None,
                });
                continue;
            }

            let algos: Vec<Algo> = entree.empreintes.iter().map(|(a, _)| *a).collect();
            let attendu: Vec<Empreinte> = entree
                .empreintes
                .iter()
                .map(|(a, v)| Empreinte {
                    algo: *a,
                    valeur: v.clone(),
                })
                .collect();

            match relire(&cible, &algos) {
                Ok(relu) => {
                    let mode = Some(relu.mode);
                    match comparer(&attendu, entree.taille, &relu) {
                        Ok(()) => lignes.push(LigneVerif {
                            fichier: entree.fichier,
                            etat: Etat::Ok,
                            detail: None,
                            mode_lecture: mode,
                        }),
                        Err(detail) => lignes.push(LigneVerif {
                            fichier: entree.fichier,
                            etat: Etat::Echec,
                            detail: Some(detail),
                            mode_lecture: mode,
                        }),
                    }
                }
                Err(e) => lignes.push(LigneVerif {
                    fichier: entree.fichier,
                    etat: Etat::Echec,
                    detail: Some(format!("relecture impossible : {e}")),
                    mode_lecture: None,
                }),
            }
        }
    }

    // Fichiers présents mais non attestés par le MHL.
    let inventaire = crate::scan::scanner(dossier)?;
    let en_trop: Vec<String> = inventaire
        .fichiers
        .iter()
        .map(|f| f.chemin.clone())
        .filter(|c| {
            !connus.contains(c)
                && !c.to_ascii_lowercase().ends_with(".mhl")
                && !c.ends_with("_rapport.json")
        })
        .collect();

    let residus: Vec<String> = crate::copy::residus(dossier)
        .into_iter()
        .map(|p| p.display().to_string())
        .collect();

    let etat = if lignes.iter().all(|l| l.etat.ok()) && !lignes.is_empty() && residus.is_empty() {
        Etat::Ok
    } else {
        Etat::Echec
    };

    Ok(RapportVerif {
        dossier: dossier.display().to_string(),
        mhls: mhls.iter().map(|p| p.display().to_string()).collect(),
        lignes,
        absents,
        en_trop,
        residus,
        etat,
    })
}
