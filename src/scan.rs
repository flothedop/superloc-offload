//! Parcours de la carte source.
//!
//! Recense les fichiers à copier, écarte les fichiers système et regroupe
//! les fichiers par clip pour les caméras qui découpent un plan en plusieurs
//! fichiers (RED notamment).

use serde::{Deserialize, Serialize};
use std::io;
use std::path::{Component, Path, PathBuf};
use std::time::SystemTime;
use walkdir::WalkDir;

/// Fichiers système à ne pas copier, mais à signaler dans le rapport.
const IGNORES: &[&str] = &[
    ".DS_Store",
    ".Spotlight-V100",
    ".fseventsd",
    ".Trashes",
    ".TemporaryItems",
    ".DocumentRevisions-V100",
    "Thumbs.db",
    "desktop.ini",
    "$RECYCLE.BIN",
    "System Volume Information",
    ".DocumentRevisions",
];

fn est_ignore(nom: &str) -> bool {
    if IGNORES.iter().any(|i| i.eq_ignore_ascii_case(nom)) {
        return true;
    }
    // Fichiers AppleDouble (fourches de ressources créées par macOS).
    nom.starts_with("._")
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FichierSource {
    /// Chemin relatif à la racine de la carte, séparateurs `/`.
    pub chemin: String,
    pub taille: u64,
    /// Date de dernière modification, ISO 8601.
    pub modifie_le: Option<String>,
    /// Clip auquel appartient ce fichier (voir [`clip_de`]).
    pub clip: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Scan {
    pub fichiers: Vec<FichierSource>,
    /// Fichiers système écartés, pour information dans le rapport.
    pub ignores: Vec<String>,
    /// Liens symboliques rencontrés : jamais suivis, jamais copiés.
    pub liens: Vec<String>,
    pub octets_total: u64,
}

impl Scan {
    pub fn nb_clips(&self) -> usize {
        let mut clips: Vec<&str> = self.fichiers.iter().map(|f| f.clip.as_str()).collect();
        clips.sort_unstable();
        clips.dedup();
        clips.len()
    }
}

/// Détermine le clip d'un fichier.
///
/// Sur RED, un plan est un dossier `.RDC` contenant plusieurs fichiers
/// `.R3D` : il manque un segment et le plan est inexploitable, donc le
/// rapport doit raisonner par dossier de clip et pas seulement par fichier.
/// Même logique pour les dossiers `.AVCHD`/`.brawf` et les paquets caméra.
pub fn clip_de(relatif: &str) -> String {
    let p = Path::new(relatif);
    let mut accumule = PathBuf::new();
    for comp in p.components() {
        if let Component::Normal(c) = comp {
            accumule.push(c);
            let nom = c.to_string_lossy().to_ascii_lowercase();
            if nom.ends_with(".rdc") || nom.ends_with(".avchd") || nom.ends_with(".brawf") {
                return accumule.to_string_lossy().replace('\\', "/");
            }
        }
    }
    relatif.to_string()
}

fn iso8601(t: SystemTime) -> Option<String> {
    let dt: chrono::DateTime<chrono::Utc> = t.into();
    Some(dt.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
}

/// Parcourt `racine` et renvoie l'inventaire des fichiers à copier.
pub fn scanner(racine: &Path) -> io::Result<Scan> {
    if !racine.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!(
                "source introuvable ou non accessible : {}",
                racine.display()
            ),
        ));
    }

    let mut scan = Scan::default();

    for entree in WalkDir::new(racine).follow_links(false).sort_by_file_name() {
        let entree = entree.map_err(|e| io::Error::other(e.to_string()))?;
        let chemin = entree.path();
        if chemin == racine {
            continue;
        }

        let nom = entree.file_name().to_string_lossy().to_string();
        let relatif = chemin
            .strip_prefix(racine)
            .map_err(|e| io::Error::other(e.to_string()))?
            .to_string_lossy()
            .replace('\\', "/");

        if est_ignore(&nom) {
            scan.ignores.push(relatif);
            continue;
        }
        // Un dossier ignoré ne doit pas être descendu : WalkDir continue,
        // on écarte donc aussi tout ce qui se trouve dessous.
        if relatif.split('/').any(est_ignore) {
            scan.ignores.push(relatif);
            continue;
        }

        let ft = entree.file_type();
        if ft.is_symlink() {
            scan.liens.push(relatif);
            continue;
        }
        if ft.is_dir() {
            continue;
        }
        if !ft.is_file() {
            continue;
        }

        let meta = entree
            .metadata()
            .map_err(|e| io::Error::other(e.to_string()))?;
        let taille = meta.len();
        scan.octets_total += taille;
        scan.fichiers.push(FichierSource {
            clip: clip_de(&relatif),
            chemin: relatif,
            taille,
            modifie_le: meta.modified().ok().and_then(iso8601),
        });
    }

    Ok(scan)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clip_red_regroupe_les_segments() {
        let a = clip_de("A001_0921XY.RDC/A001_C003_0921XY_001.R3D");
        let b = clip_de("A001_0921XY.RDC/A001_C003_0921XY_002.R3D");
        assert_eq!(a, b);
        assert_eq!(a, "A001_0921XY.RDC");
    }

    #[test]
    fn clip_red_imbrique_dans_larborescence() {
        let c = clip_de("RED/A001_0921XY.RDC/A001_C003_0921XY_001.R3D");
        assert_eq!(c, "RED/A001_0921XY.RDC");
    }

    #[test]
    fn fichier_simple_est_son_propre_clip() {
        assert_eq!(clip_de("Clip/C0001.MXF"), "Clip/C0001.MXF");
    }

    #[test]
    fn fichiers_systeme_ecartes() {
        assert!(est_ignore(".DS_Store"));
        assert!(est_ignore("._A001_C003.R3D"));
        assert!(est_ignore("Thumbs.db"));
        assert!(!est_ignore("A001_C003.R3D"));
    }
}
