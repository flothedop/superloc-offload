//! Écriture et lecture des fichiers MHL (Media Hash List) v1.1.
//!
//! Format XML normalisé, lu par Silverstack, ShotPut Pro, DaVinci Resolve et
//! la plupart des outils de DIT. Déposé sur chaque destination, il permet de
//! revérifier un disque des mois plus tard, avec cet outil ou un autre.
//!
//! Structure conforme au schéma officiel MediaHashList_v1_1.xsd :
//! `hashlist` → `creatorinfo` (username, hostname, tool, startdate,
//! finishdate) puis une suite de `hash` (file, size, lastmodificationdate,
//! empreintes, hashdate).

use crate::hash::Algo;
use crate::report::Rapport;
use quick_xml::escape::escape;
use quick_xml::events::Event;
use quick_xml::Reader;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct EntreeMhl {
    pub fichier: String,
    pub taille: u64,
    pub empreintes: Vec<(Algo, String)>,
}

/// Nom de fichier MHL : `<nom>_AAAA-MM-JJ_HHMMSS.mhl`.
pub fn nom_fichier(nom: &str, horodatage: &chrono::DateTime<chrono::Local>) -> String {
    let nom = nom
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>();
    format!("{nom}_{}.mhl", horodatage.format("%Y-%m-%d_%H%M%S"))
}

fn balise<W: Write>(w: &mut W, indent: &str, nom: &str, valeur: &str) -> io::Result<()> {
    writeln!(w, "{indent}<{nom}>{}</{nom}>", escape(valeur))
}

/// Écrit le MHL d'une destination donnée (index dans `rapport.destinations`).
///
/// Seuls les fichiers réellement vérifiés sur cette destination y figurent :
/// un MHL ne doit jamais attester d'un fichier qui n'a pas été validé.
pub fn ecrire(rapport: &Rapport, destination: usize, chemin: &Path) -> io::Result<usize> {
    let fichier = std::fs::File::create(chemin)?;
    let mut w = io::BufWriter::new(fichier);
    let maintenant = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);

    writeln!(w, r#"<?xml version="1.0" encoding="UTF-8"?>"#)?;
    writeln!(w, r#"<hashlist version="1.1">"#)?;
    writeln!(w, "  <creatorinfo>")?;
    balise(&mut w, "    ", "username", &rapport.utilisateur)?;
    balise(&mut w, "    ", "hostname", &rapport.machine)?;
    balise(&mut w, "    ", "tool", &rapport.outil)?;
    balise(&mut w, "    ", "startdate", &rapport.debut)?;
    balise(&mut w, "    ", "finishdate", &rapport.fin)?;
    writeln!(w, "  </creatorinfo>")?;

    let mut ecrits = 0usize;
    for f in &rapport.fichiers {
        let verifie = f
            .verifications
            .iter()
            .any(|v| v.destination == destination && v.etat.ok());
        if !verifie {
            continue;
        }
        writeln!(w, "  <hash>")?;
        balise(&mut w, "    ", "file", &f.chemin)?;
        balise(&mut w, "    ", "size", &f.taille.to_string())?;
        balise(
            &mut w,
            "    ",
            "lastmodificationdate",
            f.modifie_le.as_deref().unwrap_or(&rapport.debut),
        )?;
        for e in &f.empreintes {
            balise(&mut w, "    ", e.algo.mhl_element(), &e.valeur)?;
        }
        balise(&mut w, "    ", "hashdate", &maintenant)?;
        writeln!(w, "  </hash>")?;
        ecrits += 1;
    }

    writeln!(w, "</hashlist>")?;
    w.flush()?;
    w.into_inner()?.sync_all()?;
    Ok(ecrits)
}

/// Lit un fichier MHL et renvoie ses entrées.
pub fn lire(chemin: &Path) -> io::Result<Vec<EntreeMhl>> {
    let contenu = std::fs::read_to_string(chemin)?;
    let mut lecteur = Reader::from_str(&contenu);
    lecteur.config_mut().trim_text(true);

    let mut entrees: Vec<EntreeMhl> = Vec::new();
    let mut courante: Option<EntreeMhl> = None;
    let mut balise = String::new();
    let mut texte = String::new();

    loop {
        match lecteur.read_event() {
            Ok(Event::Start(e)) => {
                let nom = e.name().as_ref().to_string();
                if nom == "hash" {
                    courante = Some(EntreeMhl {
                        fichier: String::new(),
                        taille: 0,
                        empreintes: Vec::new(),
                    });
                }
                balise = nom;
                texte.clear();
            }
            Ok(Event::Text(t)) => texte.push_str(&t.xml10_content()),
            // Les entités (&amp; dans un nom de fichier, par exemple)
            // arrivent en événements distincts : on les recolle au texte.
            Ok(Event::GeneralRef(r)) => match r.resolve_char_ref() {
                Ok(Some(c)) => texte.push(c),
                _ => match r.into_inner().as_ref() {
                    "amp" => texte.push('&'),
                    "lt" => texte.push('<'),
                    "gt" => texte.push('>'),
                    "quot" => texte.push('"'),
                    "apos" => texte.push('\''),
                    autre => {
                        return Err(io::Error::other(format!(
                            "MHL illisible : entité inconnue &{autre};"
                        )))
                    }
                },
            },
            Ok(Event::End(e)) => {
                let nom = e.name().as_ref().to_string();
                if nom == "hash" {
                    if let Some(c) = courante.take() {
                        if !c.fichier.is_empty() {
                            entrees.push(c);
                        }
                    }
                } else if let Some(c) = courante.as_mut() {
                    let v = texte.trim();
                    match nom.as_str() {
                        "file" => c.fichier = v.replace('\\', "/"),
                        "size" => {
                            c.taille = v.parse().map_err(|_| {
                                io::Error::other(format!("MHL illisible : taille « {v} »"))
                            })?
                        }
                        "xxhash64be" | "xxhash64" => {
                            c.empreintes.push((Algo::Xxh64, v.to_lowercase()))
                        }
                        "md5" => c.empreintes.push((Algo::Md5, v.to_lowercase())),
                        _ => {}
                    }
                }
                balise.clear();
                texte.clear();
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(io::Error::other(format!("MHL illisible : {e}"))),
            _ => {}
        }
    }

    let _ = balise;
    Ok(entrees)
}

/// Cherche les fichiers MHL présents à la racine d'un dossier.
pub fn trouver(dossier: &Path) -> io::Result<Vec<PathBuf>> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dossier)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.is_file()
                && p.extension()
                    .map(|e| e.eq_ignore_ascii_case("mhl"))
                    .unwrap_or(false)
        })
        .collect();
    v.sort();
    Ok(v)
}
