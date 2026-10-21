//! Registre local des cartes déchargées.
//!
//! À l'insertion d'une carte, l'application peut dire « déjà déchargée le …
//! vers … ». L'empreinte d'une carte est calculée sur la liste de ses
//! fichiers (chemin, taille, date) : rapide, sans relire les rushs. Le
//! registre reste sur la machine.

use crate::hash::{self, Algo};
use crate::inventaire::Inventaire;
use crate::rapport::{Rapport, Verdict};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Entree {
    pub date: DateTime<Utc>,
    pub carte: String,
    pub empreinte_carte: String,
    pub nb_fichiers: usize,
    pub octets: u64,
    pub destinations: Vec<String>,
    pub verdict: Verdict,
}

fn empreinte<'a>(it: impl Iterator<Item = (&'a str, u64, Option<DateTime<Utc>>)>) -> String {
    let mut lignes: Vec<String> = it
        .map(|(c, t, m)| format!("{c}\0{t}\0{}", m.map(|m| m.timestamp()).unwrap_or(0)))
        .collect();
    lignes.sort();
    hash::de_donnees(Algo::Xxh128, lignes.join("\n").as_bytes())
}

pub fn empreinte_inventaire(inv: &Inventaire) -> String {
    empreinte(
        inv.fichiers
            .iter()
            .map(|f| (f.chemin.as_str(), f.taille_annoncee, f.modifie)),
    )
}

pub fn empreinte_rapport(r: &Rapport) -> String {
    empreinte(
        r.fichiers
            .iter()
            .map(|f| (f.chemin.as_str(), f.taille_annoncee, f.modifie)),
    )
}

pub struct Registre {
    chemin: PathBuf,
}

impl Registre {
    pub fn new(dossier: &Path) -> Registre {
        Registre {
            chemin: dossier.join("registre.jsonl"),
        }
    }

    pub fn entrees(&self) -> Vec<Entree> {
        std::fs::read_to_string(&self.chemin)
            .map(|t| {
                t.lines()
                    .filter_map(|l| serde_json::from_str(l).ok())
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn chercher(&self, empreinte_carte: &str) -> Vec<Entree> {
        self.entrees()
            .into_iter()
            .filter(|e| e.empreinte_carte == empreinte_carte)
            .collect()
    }

    pub fn ajouter(&self, r: &Rapport) -> std::io::Result<()> {
        if let Some(p) = self.chemin.parent() {
            std::fs::create_dir_all(p)?;
        }
        let e = Entree {
            date: r.fin,
            carte: r.nom.clone(),
            empreinte_carte: empreinte_rapport(r),
            nb_fichiers: r.nb_fichiers,
            octets: r.octets,
            destinations: r.destinations.iter().map(|d| d.dossier.clone()).collect(),
            verdict: r.verdict,
        };
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.chemin)?;
        writeln!(f, "{}", serde_json::to_string(&e)?)?;
        f.sync_all()
    }
}
