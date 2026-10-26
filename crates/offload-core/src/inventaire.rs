//! Inventaire de la carte.
//!
//! Tout est copié, fichiers de 0 octet et dossiers vides compris : certaines
//! caméras et certains logiciels reconnaissent une carte à son arborescence.
//! Seuls sont écartés les fichiers que le système d'exploitation dépose
//! lui-même sur un volume (index, corbeille, fourches AppleDouble) ; ils sont
//! consignés dans le rapport et dans le manifeste.
//!
//! La taille annoncée par le système n'est qu'indicative : sur un volume
//! Codex HDE, les fichiers affichent 0 octet tant qu'ils ne sont pas lus. La
//! copie lit toujours jusqu'à la fin réelle du fichier.

use crate::erreur::{Code, Erreur};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};
use walkdir::WalkDir;

/// Entrées déposées par les systèmes d'exploitation, jamais par une caméra.
/// `ascmhl` : historique ASC MHL d'une copie précédente, écarté comme le fait
/// l'outil de référence de l'ASC (il entrerait en conflit avec le nôtre).
/// `_offload` : rapports d'un déchargement précédent de cet outil (ils
/// seraient remplacés par les nouveaux).
pub const EXCLUSIONS: &[&str] = &[
    ".DS_Store",
    ".Spotlight-V100",
    ".fseventsd",
    ".Trashes",
    ".TemporaryItems",
    ".DocumentRevisions-V100",
    ".apdisk",
    "Thumbs.db",
    "desktop.ini",
    "$RECYCLE.BIN",
    "System Volume Information",
    "ascmhl",
    "_offload",
];

pub fn est_exclu(nom: &str) -> bool {
    EXCLUSIONS.iter().any(|i| i.eq_ignore_ascii_case(nom)) || nom.starts_with("._")
}

/// Motifs d'exclusion, au format gitignore du manifeste ASC MHL.
pub fn motifs_exclusion() -> Vec<String> {
    let mut m: Vec<String> = EXCLUSIONS
        .iter()
        .filter(|s| **s != "_offload")
        .map(|s| s.to_string())
        .collect();
    m.push("ascmhl/".into());
    m.push("._*".into());
    m
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FichierCarte {
    /// Chemin relatif, séparateurs `/`, tel que lu sur la carte.
    pub chemin: String,
    /// Taille annoncée par le système au moment de l'inventaire.
    pub taille_annoncee: u64,
    pub modifie: Option<DateTime<Utc>>,
    pub cree: Option<DateTime<Utc>>,
    pub clip: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DossierCarte {
    pub chemin: String,
    pub modifie: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Inventaire {
    pub racine: PathBuf,
    pub fichiers: Vec<FichierCarte>,
    /// Tous les dossiers, parents avant enfants.
    pub dossiers: Vec<DossierCarte>,
    pub exclus: Vec<String>,
    /// Liens symboliques : jamais suivis, jamais copiés.
    pub liens: Vec<String>,
    /// Entrées impossibles à inventorier : elles ne seront PAS copiées.
    pub illisibles: Vec<Erreur>,
    pub octets_annonces: u64,
}

impl Inventaire {
    pub fn nb_clips(&self) -> usize {
        let mut c: Vec<&str> = self.fichiers.iter().map(|f| f.clip.as_str()).collect();
        c.sort_unstable();
        c.dedup();
        c.len()
    }

    /// Dossiers sans aucun fichier ni sous-dossier.
    pub fn dossiers_vides(&self) -> Vec<&str> {
        self.dossiers
            .iter()
            .map(|d| d.chemin.as_str())
            .filter(|d| {
                let prefixe = format!("{d}/");
                !self.fichiers.iter().any(|f| f.chemin.starts_with(&prefixe))
                    && !self.dossiers.iter().any(|x| x.chemin.starts_with(&prefixe))
            })
            .collect()
    }

    /// Clips dont la numérotation des segments présente un trou (un segment
    /// R3D manquant rend le plan inexploitable).
    pub fn segments_manquants(&self) -> Vec<String> {
        let mut series: BTreeMap<String, Vec<u32>> = BTreeMap::new();
        for f in &self.fichiers {
            if let Some((base, n)) = segment(&f.chemin) {
                series.entry(base).or_default().push(n);
            }
        }
        let mut manques = Vec::new();
        for (base, mut nums) in series {
            nums.sort_unstable();
            nums.dedup();
            let premier = nums[0];
            for (i, n) in nums.iter().enumerate() {
                let attendu = premier + i as u32;
                if *n != attendu {
                    manques.push(format!("{base}_{attendu:03}"));
                    break;
                }
            }
            if premier > 1 {
                manques.push(format!("{base}_001"));
            }
        }
        manques
    }
}

/// `A001_C003_0921XY_002.R3D` → (`…/A001_C003_0921XY`, 2).
fn segment(chemin: &str) -> Option<(String, u32)> {
    let (sans_ext, ext) = chemin.rsplit_once('.')?;
    if !ext.eq_ignore_ascii_case("r3d") {
        return None;
    }
    let (base, num) = sans_ext.rsplit_once('_')?;
    if num.len() != 3 || !num.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some((base.to_string(), num.parse().ok()?))
}

/// Clip d'un fichier : sur RED, un plan est un dossier `.RDC` qui contient
/// plusieurs segments `.R3D` ; même logique pour les paquets `.AVCHD` et
/// `.brawf`. Ailleurs, un fichier est son propre clip, au nom d'extension
/// près (le proxy `C0001.MP4` et le `C0001.XML` vont avec `C0001.MXF`).
pub fn clip_de(relatif: &str) -> String {
    let mut accumule = PathBuf::new();
    for comp in Path::new(relatif).components() {
        if let Component::Normal(c) = comp {
            accumule.push(c);
            let nom = c.to_string_lossy().to_ascii_lowercase();
            if nom.ends_with(".rdc") || nom.ends_with(".avchd") || nom.ends_with(".brawf") {
                return relatif_lossy(&accumule);
            }
        }
    }
    relatif.to_string()
}

/// Chemin relatif avec séparateurs `/`, composant par composant (un « \\ »
/// est un caractère permis dans un nom sous macOS et Linux). `None` si un nom
/// n'est pas de l'UTF-8 valide.
pub fn relatif(p: &Path) -> Option<String> {
    let mut parts = Vec::new();
    for c in p.components() {
        if let Component::Normal(n) = c {
            parts.push(n.to_str()?);
        }
    }
    Some(parts.join("/"))
}

fn relatif_lossy(p: &Path) -> String {
    p.components()
        .filter_map(|c| match c {
            Component::Normal(n) => Some(n.to_string_lossy().to_string()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn date(t: std::io::Result<std::time::SystemTime>) -> Option<DateTime<Utc>> {
    t.ok().map(DateTime::<Utc>::from)
}

/// Parcourt la carte.
pub fn inventorier(racine: &Path) -> Result<Inventaire, Erreur> {
    if !racine.is_dir() {
        return Err(Erreur::new(
            Code::SourceIntrouvable,
            racine.display().to_string(),
        ));
    }
    let mut inv = Inventaire {
        racine: racine.to_path_buf(),
        ..Default::default()
    };

    let mut marche = WalkDir::new(racine)
        .follow_links(false)
        .sort_by_file_name()
        .into_iter();

    while let Some(entree) = marche.next() {
        let entree = match entree {
            Ok(e) => e,
            Err(e) => {
                let chemin = e
                    .path()
                    .and_then(|p| p.strip_prefix(racine).ok())
                    .map(relatif_lossy)
                    .unwrap_or_default();
                inv.illisibles
                    .push(Erreur::new(Code::LectureSource, format!("{chemin} : {e}")));
                continue;
            }
        };
        if entree.depth() > 0 && est_exclu(&entree.file_name().to_string_lossy()) {
            if let Ok(rel) = entree.path().strip_prefix(racine) {
                inv.exclus.push(relatif_lossy(rel));
            }
            if entree.file_type().is_dir() {
                marche.skip_current_dir();
            }
            continue;
        }
        if entree.depth() == 0 {
            continue;
        }
        let Ok(rel) = entree.path().strip_prefix(racine) else {
            continue;
        };
        let Some(relatif) = relatif(rel) else {
            inv.illisibles.push(Erreur::new(
                Code::NomIllisible,
                rel.to_string_lossy().to_string(),
            ));
            continue;
        };

        let ft = entree.file_type();
        if ft.is_symlink() {
            inv.liens.push(relatif);
            continue;
        }
        // Lues sur l'entrée elle-même : sous Windows, les métadonnées de
        // l'énumération du dossier sont une copie que NTFS met à jour en
        // différé (dates de dossiers en retard).
        let meta = match std::fs::symlink_metadata(entree.path()) {
            Ok(m) => m,
            Err(e) => {
                inv.illisibles
                    .push(Erreur::new(Code::LectureSource, format!("{relatif} : {e}")));
                continue;
            }
        };
        if ft.is_dir() {
            inv.dossiers.push(DossierCarte {
                chemin: relatif,
                modifie: date(meta.modified()),
            });
        } else if ft.is_file() {
            inv.octets_annonces += meta.len();
            inv.fichiers.push(FichierCarte {
                clip: clip_de(&relatif),
                taille_annoncee: meta.len(),
                modifie: date(meta.modified()),
                cree: date(meta.created()),
                chemin: relatif,
            });
        } else {
            // Tube, socket, périphérique : jamais sur une carte caméra. Non
            // copié, et signalé (la carte ne peut pas être déclarée sauvée).
            inv.illisibles.push(Erreur::new(
                Code::LectureSource,
                format!("{relatif} : fichier spécial non copié"),
            ));
        }
    }
    Ok(inv)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn clip_red_regroupe_les_segments() {
        assert_eq!(
            clip_de("A001_0921XY.RDM/A001_C003_0921XY.RDC/A001_C003_0921XY_002.R3D"),
            "A001_0921XY.RDM/A001_C003_0921XY.RDC"
        );
        assert_eq!(clip_de("Clip/C0001.MXF"), "Clip/C0001.MXF");
    }

    #[test]
    fn exclusions_systeme() {
        assert!(est_exclu(".DS_Store"));
        assert!(est_exclu("._A001.R3D"));
        assert!(est_exclu("System Volume Information"));
        assert!(!est_exclu("A001.R3D"));
        assert!(!est_exclu(".RDM"));
    }

    #[test]
    fn un_segment_manquant_est_repere() {
        let mut inv = Inventaire::default();
        for n in [1, 2, 4] {
            inv.fichiers.push(FichierCarte {
                chemin: format!("A.RDC/A001_C003_{n:03}.R3D"),
                taille_annoncee: 1,
                modifie: None,
                cree: None,
                clip: "A.RDC".into(),
            });
        }
        assert_eq!(inv.segments_manquants(), vec!["A.RDC/A001_C003_003"]);
    }

    #[test]
    fn inventaire_complet_avec_vides_et_exclus() {
        let d = tempfile::tempdir().unwrap();
        let r = d.path();
        fs::create_dir_all(r.join("A001.RDM/A001_C001.RDC")).unwrap();
        fs::create_dir_all(r.join("VIDE/SOUS")).unwrap();
        fs::create_dir_all(r.join(".Spotlight-V100/Store")).unwrap();
        fs::write(r.join(".Spotlight-V100/Store/x"), b"x").unwrap();
        fs::write(r.join("A001.RDM/A001_C001.RDC/A001_C001_001.R3D"), b"abc").unwrap();
        fs::write(r.join("A001.RDM/zero.txt"), b"").unwrap();
        fs::write(r.join("._zero.txt"), b"apple").unwrap();
        let inv = inventorier(r).unwrap();
        let chemins: Vec<&str> = inv.fichiers.iter().map(|f| f.chemin.as_str()).collect();
        assert_eq!(
            chemins,
            vec![
                "A001.RDM/A001_C001.RDC/A001_C001_001.R3D",
                "A001.RDM/zero.txt"
            ]
        );
        assert_eq!(inv.exclus, vec![".Spotlight-V100", "._zero.txt"]);
        assert_eq!(inv.dossiers_vides(), vec!["VIDE/SOUS"]);
        assert_eq!(inv.octets_annonces, 3);
    }
}
