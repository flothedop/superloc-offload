//! Vérification d'un dossier déjà déchargé, à partir de ses manifestes.
//!
//! Sert à revérifier un disque après l'avoir éjecté et rebranché (seule
//! façon de prouver que les données ont quitté le cache du disque lui-même),
//! ou des mois plus tard. Lecture seule : rien n'est écrit dans le dossier.

use crate::ascmhl;
use crate::erreur::{Code, Erreur};
use crate::hash::{self, Algo, Hacheur};
use crate::inventaire;
use crate::mhl1;
use crate::moteur::Annulation;
use crate::sanscache;
use crate::{BLOC, DOSSIER_RAPPORT};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::path::Path;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Probleme {
    pub chemin: String,
    pub erreur: Erreur,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Bilan {
    pub dossier: String,
    /// « ascmhl » ou « mhl-v1.1 ».
    pub format: String,
    pub manifestes: Vec<String>,
    pub nb_fichiers: usize,
    pub octets: u64,
    pub conformes: usize,
    pub problemes: Vec<Probleme>,
    /// Présents sur le disque, absents du manifeste.
    pub inattendus: Vec<String>,
    pub debut: DateTime<Utc>,
    pub fin: DateTime<Utc>,
    pub annule: bool,
    pub lecture_directe: bool,
}

impl Bilan {
    pub fn conforme(&self) -> bool {
        !self.annule
            && self.problemes.is_empty()
            && self.inattendus.is_empty()
            && self.nb_fichiers > 0
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Progres {
    pub index: usize,
    pub nb_fichiers: usize,
    pub chemin: String,
    pub octets: u64,
    pub total: u64,
}

/// Motif gitignore simplifié : `nom`, `nom/` (dossier), `*.ext`, `._*`.
pub fn correspond(motif: &str, chemin: &str) -> bool {
    let dossier_seul = motif.ends_with('/');
    let m = motif.trim_end_matches('/').trim_start_matches('/');
    let composants: Vec<&str> = chemin.split('/').collect();
    let n = composants.len();
    composants.iter().enumerate().any(|(i, c)| {
        // Un motif « dossier » ne s'applique qu'aux composants qui ont une
        // suite (donc qui sont des dossiers).
        if dossier_seul && i == n - 1 {
            return false;
        }
        glob(m, c)
    })
}

fn glob(motif: &str, nom: &str) -> bool {
    match motif.split_once('*') {
        None => motif.eq_ignore_ascii_case(nom) || motif == nom,
        Some((avant, apres)) => {
            nom.len() >= avant.len() + apres.len() && nom.starts_with(avant) && nom.ends_with(apres)
        }
    }
}

struct Attendu {
    taille: u64,
    empreintes: Vec<hash::Empreinte>,
}

/// Format, manifestes lus, fichiers attendus, motifs d'exclusion.
type Charge = (String, Vec<String>, BTreeMap<String, Attendu>, Vec<String>);

fn charger(dossier: &Path) -> Result<Charge, Erreur> {
    if dossier.join(ascmhl::DOSSIER).join(ascmhl::CHAINE).is_file() {
        let generations = ascmhl::lire_historique(dossier)?;
        let mut attendus: BTreeMap<String, Attendu> = BTreeMap::new();
        let mut ignores: Vec<String> = Vec::new();
        let mut noms = Vec::new();
        for g in &generations {
            noms.push(
                g.chemin
                    .strip_prefix(dossier)
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_default(),
            );
            ignores.extend(g.ignores.iter().cloned());
            for e in &g.entrees {
                if e.action == "failed" {
                    continue;
                }
                attendus.insert(
                    e.chemin.clone(),
                    Attendu {
                        taille: e.taille,
                        empreintes: e.empreintes.clone(),
                    },
                );
            }
        }
        return Ok(("ascmhl".into(), noms, attendus, ignores));
    }
    let manifestes = mhl1::trouver(dossier);
    if manifestes.is_empty() {
        return Err(Erreur::new(
            Code::ManifesteAbsent,
            dossier.display().to_string(),
        ));
    }
    let mut attendus = BTreeMap::new();
    let mut noms = Vec::new();
    for m in &manifestes {
        noms.push(
            m.file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default(),
        );
        for e in mhl1::lire(m)? {
            attendus.insert(
                e.chemin.clone(),
                Attendu {
                    taille: e.taille,
                    empreintes: e.empreintes,
                },
            );
        }
    }
    let mut ignores = inventaire::motifs_exclusion();
    ignores.extend(noms.iter().cloned());
    Ok(("mhl-v1.1".into(), noms, attendus, ignores))
}

fn rehacher(
    chemin: &Path,
    algo: Algo,
    octets: &mut u64,
    annulation: &Annulation,
) -> Result<(String, u64, bool), Erreur> {
    let mut l = sanscache::ouvrir(chemin, BLOC)
        .map_err(|e| Erreur::io(Code::RelectureImpossible, chemin.display(), &e))?;
    let direct = l.mode().garantit_le_support();
    let mut h = Hacheur::new(algo);
    let mut n = 0u64;
    loop {
        if annulation.demandee() {
            return Err(Erreur::new(Code::Annule, ""));
        }
        let b = l
            .lire_bloc()
            .map_err(|e| Erreur::io(Code::RelectureImpossible, chemin.display(), &e))?;
        if b.is_empty() {
            break;
        }
        n += b.len() as u64;
        *octets += b.len() as u64;
        h.update(b);
    }
    Ok((h.finir(), n, direct))
}

pub fn verifier(
    dossier: &Path,
    annulation: &Annulation,
    progres: &mut dyn FnMut(Progres),
) -> Result<Bilan, Erreur> {
    let debut = Utc::now();
    let (format, manifestes, attendus, mut ignores) = charger(dossier)?;
    ignores.push(format!("{DOSSIER_RAPPORT}/"));
    ignores.push("ascmhl/".into());

    let total: u64 = attendus.values().map(|a| a.taille).sum();
    let mut bilan = Bilan {
        dossier: dossier.display().to_string(),
        format,
        manifestes,
        nb_fichiers: attendus.len(),
        octets: total,
        conformes: 0,
        problemes: Vec::new(),
        inattendus: Vec::new(),
        debut,
        fin: debut,
        annule: false,
        lecture_directe: true,
    };

    let mut octets = 0u64;
    let nb = attendus.len();
    for (i, (chemin, a)) in attendus.iter().enumerate() {
        if annulation.demandee() {
            bilan.annule = true;
            break;
        }
        progres(Progres {
            index: i,
            nb_fichiers: nb,
            chemin: chemin.clone(),
            octets,
            total,
        });
        let p = chemin
            .split('/')
            .fold(dossier.to_path_buf(), |p, s| p.join(s));
        let pb = |code: Code, detail: String| Probleme {
            chemin: chemin.clone(),
            erreur: Erreur::new(code, detail),
        };
        let Ok(meta) = std::fs::metadata(&p) else {
            bilan.problemes.push(pb(Code::Manquant, chemin.clone()));
            continue;
        };
        if meta.len() != a.taille {
            bilan.problemes.push(pb(
                Code::TailleDifferente,
                format!("{chemin} : {} octets au lieu de {}", meta.len(), a.taille),
            ));
            continue;
        }
        let algos: Vec<Algo> = a.empreintes.iter().map(|e| e.algo).collect();
        if algos.is_empty() {
            bilan.problemes.push(pb(
                Code::ManifesteIllisible,
                format!("{chemin} : aucune empreinte"),
            ));
            continue;
        }
        let algo = hash::de_comparaison(&algos);
        let attendu = a
            .empreintes
            .iter()
            .find(|e| e.algo == algo)
            .map(|e| e.valeur.to_ascii_lowercase())
            .unwrap_or_default();
        match rehacher(&p, algo, &mut octets, annulation) {
            Ok((v, _, direct)) => {
                bilan.lecture_directe &= direct;
                if v == attendu {
                    bilan.conformes += 1;
                } else {
                    bilan.problemes.push(pb(
                        Code::EmpreinteDifferente,
                        format!("{chemin} : {} {v} au lieu de {attendu}", algo.nom()),
                    ));
                }
            }
            Err(e) if e.code == Code::Annule => {
                bilan.annule = true;
                break;
            }
            Err(e) => bilan.problemes.push(Probleme {
                chemin: chemin.clone(),
                erreur: e,
            }),
        }
    }

    // Fichiers inattendus (hors manifestes et exclusions).
    let connus: HashSet<&str> = attendus.keys().map(String::as_str).collect();
    for e in walkdir::WalkDir::new(dossier)
        .follow_links(false)
        .sort_by_file_name()
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
    {
        let Ok(rel) = e.path().strip_prefix(dossier) else {
            continue;
        };
        let Some(rel) = inventaire::relatif(rel) else {
            continue;
        };
        if connus.contains(rel.as_str()) || ignores.iter().any(|m| correspond(m, &rel)) {
            continue;
        }
        bilan.inattendus.push(rel);
    }
    bilan.fin = Utc::now();
    Ok(bilan)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn motifs_dignorance() {
        assert!(correspond("ascmhl/", "ascmhl/0001.mhl"));
        assert!(!correspond("ascmhl/", "ascmhl"));
        assert!(correspond(".DS_Store", "A/.DS_Store"));
        assert!(correspond("._*", "A/._clip.mov"));
        assert!(correspond("_offload/", "_offload/rapport.html"));
        assert!(correspond(
            "A001_2026-10-21_101010.mhl",
            "A001_2026-10-21_101010.mhl"
        ));
        assert!(!correspond(".DS_Store", "A/clip.mov"));
    }
}
