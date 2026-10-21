//! MHL v1.1 (format historique).
//!
//! Toujours demandé par Netflix (xxHash64 big-endian) et lu par OffShoot,
//! YoYotta, Resolve. Écrit à la racine du déchargement, à côté du dossier
//! `ascmhl/`. Seuls md5, sha1 et xxh64 existent dans ce format.

use crate::ascmhl::Entree;
use crate::erreur::{Code, Erreur};
use crate::hash::{Algo, Empreinte};
use crate::xml::{self, esc};
use chrono::{DateTime, SecondsFormat, Utc};
use std::io::Write;
use std::path::{Path, PathBuf};

pub fn nom_fichier(dossier: &str, debut: &DateTime<Utc>) -> String {
    format!("{dossier}_{}.mhl", debut.format("%Y-%m-%d_%H%M%S"))
}

fn d(x: &DateTime<Utc>) -> String {
    x.to_rfc3339_opts(SecondsFormat::Secs, true)
}

pub struct Infos<'a> {
    pub nom: &'a str,
    pub utilisateur: &'a str,
    pub machine: &'a str,
    pub outil: &'a str,
    pub debut: DateTime<Utc>,
    pub fin: DateTime<Utc>,
}

pub fn texte(infos: &Infos, fichiers: &[Entree]) -> String {
    let mut x = String::new();
    x.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    x.push_str("<hashlist version=\"1.1\">\n");
    x.push_str("  <creatorinfo>\n");
    if !infos.nom.trim().is_empty() {
        x.push_str(&format!("    <name>{}</name>\n", esc(infos.nom)));
    }
    x.push_str(&format!(
        "    <username>{}</username>\n",
        esc(infos.utilisateur)
    ));
    x.push_str(&format!(
        "    <hostname>{}</hostname>\n",
        esc(infos.machine)
    ));
    x.push_str(&format!("    <tool>{}</tool>\n", esc(infos.outil)));
    x.push_str(&format!("    <startdate>{}</startdate>\n", d(&infos.debut)));
    x.push_str(&format!("    <finishdate>{}</finishdate>\n", d(&infos.fin)));
    x.push_str("  </creatorinfo>\n");
    for f in fichiers {
        x.push_str("  <hash>\n");
        x.push_str(&format!("    <file>{}</file>\n", esc(&f.chemin)));
        x.push_str(&format!("    <size>{}</size>\n", f.taille));
        if let Some(c) = &f.cree {
            x.push_str(&format!("    <creationdate>{}</creationdate>\n", d(c)));
        }
        if let Some(m) = &f.modifie {
            x.push_str(&format!(
                "    <lastmodificationdate>{}</lastmodificationdate>\n",
                d(m)
            ));
        }
        // Une seule empreinte par entrée, comme les outils qui lisent ce
        // format l'attendent : xxhash64be de préférence (Netflix), sinon md5,
        // sinon sha1.
        if let Some(e) = [Algo::Xxh64, Algo::Md5, Algo::Sha1]
            .iter()
            .find_map(|a| f.empreintes.iter().find(|e| e.algo == *a))
        {
            let nom = e.algo.nom_mhl1().unwrap_or("xxhash64be");
            x.push_str(&format!("    <{nom}>{}</{nom}>\n", e.valeur));
        }
        x.push_str(&format!(
            "    <hashdate>{}</hashdate>\n",
            d(&f.date_empreinte)
        ));
        x.push_str("  </hash>\n");
    }
    x.push_str("</hashlist>\n");
    x
}

/// Vrai si au moins un algorithme demandé existe en MHL v1.1.
pub fn possible(algos: &[Algo]) -> bool {
    algos.iter().any(|a| a.nom_mhl1().is_some())
}

pub fn ecrire(racine: &Path, infos: &Infos, fichiers: &[Entree]) -> Result<String, Erreur> {
    let feuille = racine
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "offload".into());
    let nom = nom_fichier(&feuille, &infos.debut);
    let chemin = racine.join(&nom);
    let err = |e: std::io::Error| Erreur::io(Code::EcritureImpossible, chemin.display(), &e);
    let mut f = crate::es::creer_exclusif(&chemin).map_err(err)?;
    f.write_all(texte(infos, fichiers).as_bytes())
        .map_err(err)?;
    crate::es::synchroniser(&f).map_err(err)?;
    Ok(nom)
}

pub fn lire(chemin: &Path) -> Result<Vec<Entree>, Erreur> {
    let t = std::fs::read_to_string(chemin)
        .map_err(|e| Erreur::io(Code::ManifesteIllisible, chemin.display(), &e))?;
    let r = xml::lire(&t).map_err(|e| {
        Erreur::new(
            Code::ManifesteIllisible,
            format!("{} : {e}", chemin.display()),
        )
    })?;
    if r.nom != "hashlist" {
        return Err(Erreur::new(
            Code::ManifesteIllisible,
            chemin.display().to_string(),
        ));
    }
    let date = |s: Option<&str>| {
        s.and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|d| d.with_timezone(&Utc))
    };
    let mut v = Vec::new();
    for h in r.enfants("hash") {
        let Some(fichier) = h.texte_de("file") else {
            continue;
        };
        let mut empreintes = Vec::new();
        for (balise, algo) in [
            ("md5", Algo::Md5),
            ("sha1", Algo::Sha1),
            ("xxhash64be", Algo::Xxh64),
            ("xxhash64", Algo::Xxh64),
        ] {
            if let Some(val) = h.texte_de(balise) {
                if !empreintes.iter().any(|e: &Empreinte| e.algo == algo) {
                    empreintes.push(Empreinte {
                        algo,
                        valeur: val.to_ascii_lowercase(),
                    });
                }
            }
        }
        v.push(Entree {
            chemin: fichier.replace('\\', "/"),
            taille: h.texte_de("size").and_then(|s| s.parse().ok()).unwrap_or(0),
            modifie: date(h.texte_de("lastmodificationdate")),
            cree: date(h.texte_de("creationdate")),
            empreintes,
            date_empreinte: date(h.texte_de("hashdate")).unwrap_or_default(),
            action: "original",
        });
    }
    Ok(v)
}

/// Manifestes v1.1 présents à la racine d'un dossier.
pub fn trouver(dossier: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dossier)
        .map(|it| {
            it.filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| {
                    p.is_file()
                        && p.extension()
                            .map(|e| e.eq_ignore_ascii_case("mhl"))
                            .unwrap_or(false)
                })
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aller_retour() {
        let dir = tempfile::tempdir().unwrap();
        let racine = dir.path().join("A001");
        std::fs::create_dir(&racine).unwrap();
        let e = Entree {
            chemin: "Clip/a&b.mov".into(),
            taille: 3,
            modifie: Some(Utc::now()),
            cree: None,
            empreintes: vec![
                Empreinte {
                    algo: Algo::Xxh128,
                    valeur: "06b05ab6733a618578af5f94892f3950".into(),
                },
                Empreinte {
                    algo: Algo::Xxh64,
                    valeur: "44bc2cf5ad770999".into(),
                },
            ],
            date_empreinte: Utc::now(),
            action: "original",
        };
        let infos = Infos {
            nom: "A001",
            utilisateur: "flo",
            machine: "poste",
            outil: "SuperLoc Offload 0.2.0",
            debut: Utc::now(),
            fin: Utc::now(),
        };
        let nom = ecrire(&racine, &infos, std::slice::from_ref(&e)).unwrap();
        let t = std::fs::read_to_string(racine.join(&nom)).unwrap();
        assert!(t.contains("<xxhash64be>44bc2cf5ad770999</xxhash64be>"));
        assert!(!t.contains("xxh128"), "xxh128 n'existe pas en v1.1");
        let lu = lire(&racine.join(&nom)).unwrap();
        assert_eq!(lu[0].chemin, "Clip/a&b.mov");
        assert_eq!(lu[0].empreintes[0].algo, Algo::Xxh64);
        assert_eq!(trouver(&racine).len(), 1);
    }
}
