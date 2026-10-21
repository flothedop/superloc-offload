//! Nommage des dossiers de déchargement.
//!
//! Gabarit à jetons : `{carte}`, `{date}`, `{projet}`, `{jour}`, `{camera}`,
//! `{operateur}`. Un « / » crée un sous-dossier. Un jeton vide disparaît avec
//! le séparateur qui le précède. Exemple : `{projet}/{date}/{carte}`.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const PAR_DEFAUT: &str = "{date}_{carte}";

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Valeurs {
    pub carte: String,
    /// AAAA-MM-JJ.
    pub date: String,
    pub projet: String,
    pub jour: String,
    pub camera: String,
    pub operateur: String,
}

/// Rend un nom utilisable dans un chemin sur tous les systèmes.
pub fn assainir(s: &str) -> String {
    let propre: String = s
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '-',
            c if (c as u32) < 32 => '-',
            c => c,
        })
        .collect();
    propre
        .trim()
        .trim_end_matches('.')
        .trim_start_matches('.')
        .trim()
        .to_string()
}

/// Chemin relatif obtenu à partir du gabarit.
pub fn rendre(gabarit: &str, v: &Valeurs) -> String {
    let gabarit = if gabarit.trim().is_empty() {
        PAR_DEFAUT
    } else {
        gabarit
    };
    let mut segments = Vec::new();
    for segment in gabarit.split('/') {
        let mut s = String::new();
        let mut reste = segment;
        while let Some(i) = reste.find('{') {
            let (avant, apres) = reste.split_at(i);
            s.push_str(avant);
            let Some(j) = apres.find('}') else {
                s.push_str(apres);
                reste = "";
                break;
            };
            let jeton = &apres[1..j];
            let valeur = match jeton {
                "carte" => &v.carte,
                "date" => &v.date,
                "projet" => &v.projet,
                "jour" => &v.jour,
                "camera" => &v.camera,
                "operateur" => &v.operateur,
                _ => "",
            };
            let valeur = assainir(valeur);
            if valeur.is_empty() {
                // Le séparateur qui précédait le jeton vide disparaît aussi.
                while s.ends_with(['_', '-', ' ', '.']) {
                    s.pop();
                }
            } else {
                s.push_str(&valeur);
            }
            reste = &apres[j + 1..];
        }
        s.push_str(reste);
        let s = assainir(s.trim_start_matches(['_', '-', ' ']));
        if !s.is_empty() && s != ".." {
            segments.push(s);
        }
    }
    if segments.is_empty() {
        let c = assainir(&v.carte);
        segments.push(if c.is_empty() { "CARTE".into() } else { c });
    }
    segments.join("/")
}

/// Premier nom libre sur TOUTES les destinations : `nom`, `nom_2`, `nom_3`…
pub fn nom_libre(bases: &[PathBuf], nom: &str) -> String {
    let libre = |n: &str| bases.iter().all(|b| !b.join(n).exists());
    if libre(nom) {
        return nom.to_string();
    }
    (2..10_000)
        .map(|i| format!("{nom}_{i}"))
        .find(|n| libre(n))
        .unwrap_or_else(|| format!("{nom}_{}", chrono::Utc::now().timestamp()))
}

/// Dossier final de chaque destination.
pub fn dossiers(bases: &[PathBuf], relatif: &str) -> Vec<PathBuf> {
    bases
        .iter()
        .map(|b| {
            relatif
                .split('/')
                .fold(b.clone(), |p: PathBuf, s| p.join(s))
        })
        .collect()
}

/// Nom du dossier de déchargement (dernier composant).
pub fn feuille(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v() -> Valeurs {
        Valeurs {
            carte: "A001".into(),
            date: "2026-10-21".into(),
            projet: "Pub Lille".into(),
            ..Default::default()
        }
    }

    #[test]
    fn gabarit_par_defaut() {
        assert_eq!(rendre("", &v()), "2026-10-21_A001");
    }

    #[test]
    fn sous_dossiers_et_jetons_vides() {
        assert_eq!(
            rendre("{projet}/{date}/{carte}", &v()),
            "Pub Lille/2026-10-21/A001"
        );
        assert_eq!(rendre("{projet}/J{jour}/{carte}", &v()), "Pub Lille/J/A001");
        assert_eq!(rendre("{date}_{camera}_{carte}", &v()), "2026-10-21_A001");
        assert_eq!(rendre("{camera}/{carte}", &v()), "A001");
    }

    #[test]
    fn les_caracteres_dangereux_sont_neutralises() {
        let mut x = v();
        x.carte = "../A:001".into();
        assert_eq!(rendre("{carte}", &x), "A-001");
        assert_eq!(rendre("../{carte}", &v()), "A001");
    }

    #[test]
    fn nom_libre_ajoute_un_suffixe() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir(d.path().join("A001")).unwrap();
        std::fs::create_dir(d.path().join("A001_2")).unwrap();
        assert_eq!(nom_libre(&[d.path().to_path_buf()], "A001"), "A001_3");
        assert_eq!(nom_libre(&[d.path().to_path_buf()], "A002"), "A002");
    }
}
