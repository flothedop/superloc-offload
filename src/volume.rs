//! Caractéristiques du volume qui porte un dossier.
//!
//! Ne sont retenues que les informations réellement fiables : système de
//! fichiers, caractère amovible, volume réseau. Le type de support
//! (SSD ou disque mécanique) est délibérément écarté : les bibliothèques
//! le rapportent faux dès qu'il y a une couche de virtualisation, et une
//! information fausse dans un diagnostic est pire que pas d'information.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use sysinfo::Disks;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Volume {
    pub point_montage: String,
    pub systeme_fichiers: String,
    pub amovible: bool,
    pub reseau: bool,
    pub total: u64,
    pub libre: u64,
}

impl Volume {
    /// Systèmes de fichiers coûteux en métadonnées quand il y a beaucoup de
    /// petits fichiers.
    pub fn couteux_en_petits_fichiers(&self) -> bool {
        let f = self.systeme_fichiers.to_ascii_lowercase();
        f.contains("exfat") || f.contains("fat32") || f.contains("vfat") || f.contains("msdos")
    }
}

fn est_reseau(systeme: &str, chemin: &Path) -> bool {
    let f = systeme.to_ascii_lowercase();
    let reseau = [
        "smb", "cifs", "nfs", "afp", "webdav", "sshfs", "ftp", "9p", "davfs",
    ];
    if reseau.iter().any(|r| f.contains(r)) {
        return true;
    }
    // Chemin UNC sous Windows : \\serveur\partage ou \\?\UNC\serveur\partage.
    // Un chemin « verbatim » local (\\?\C:\...) commence aussi par deux
    // barres obliques inverses mais n'a rien d'un volume réseau.
    let s = chemin.to_string_lossy();
    if let Some(reste) = s.strip_prefix("\\\\?\\") {
        return reste.to_ascii_uppercase().starts_with("UNC\\");
    }
    s.starts_with("\\\\") || s.starts_with("//")
}

/// Renvoie le volume qui porte `chemin`, c'est-à-dire le point de montage
/// le plus spécifique dont il dépend.
pub fn du_chemin(chemin: &Path) -> Option<Volume> {
    // Sous Windows, canonicalize() renvoie un chemin « verbatim » (\\?\D:\...)
    // qui ne commence jamais par un point de montage ordinaire (D:\) : la
    // comparaison échouait toujours et aucun volume n'était trouvé. dunce
    // renvoie la forme usuelle quand elle existe.
    let absolu = dunce::canonicalize(chemin).unwrap_or_else(|_| chemin.to_path_buf());

    let disques = Disks::new_with_refreshed_list();
    let mut meilleur: Option<(usize, Volume)> = None;

    for d in disques.list() {
        let point: PathBuf = d.mount_point().to_path_buf();
        if !absolu.starts_with(&point) {
            continue;
        }
        let longueur = point.components().count();
        let systeme = d.file_system().to_string_lossy().to_string();
        let v = Volume {
            point_montage: point.display().to_string(),
            reseau: est_reseau(&systeme, &point),
            systeme_fichiers: systeme,
            amovible: d.is_removable(),
            total: d.total_space(),
            libre: d.available_space(),
        };
        if meilleur
            .as_ref()
            .map(|(l, _)| longueur > *l)
            .unwrap_or(true)
        {
            meilleur = Some((longueur, v));
        }
    }

    meilleur.map(|(_, v)| v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reconnait_un_volume_reseau_au_systeme_de_fichiers() {
        assert!(est_reseau("smbfs", Path::new("/Volumes/partage")));
        assert!(est_reseau("nfs", Path::new("/mnt/nas")));
        assert!(!est_reseau("apfs", Path::new("/Volumes/RAID")));
        assert!(!est_reseau("ext4", Path::new("/")));
    }

    #[test]
    fn reconnait_un_chemin_unc_windows() {
        assert!(est_reseau("ntfs", Path::new(r"\\serveur\rushs")));
        assert!(est_reseau("ntfs", Path::new(r"\\?\UNC\serveur\rushs")));
    }

    #[test]
    fn un_chemin_verbatim_local_nest_pas_reseau() {
        // Forme renvoyée par canonicalize() sous Windows pour un disque local.
        assert!(!est_reseau("ntfs", Path::new(r"\\?\D:\rushs")));
    }

    #[test]
    fn exfat_est_signale_comme_couteux() {
        let v = |f: &str| Volume {
            point_montage: "/".into(),
            systeme_fichiers: f.into(),
            amovible: false,
            reseau: false,
            total: 0,
            libre: 0,
        };
        assert!(v("exfat").couteux_en_petits_fichiers());
        assert!(v("ExFAT").couteux_en_petits_fichiers());
        assert!(!v("apfs").couteux_en_petits_fichiers());
        assert!(!v("NTFS").couteux_en_petits_fichiers());
    }

    #[test]
    fn trouve_le_volume_du_dossier_courant() {
        let v = du_chemin(Path::new(".")).expect("un volume doit porter le dossier courant");
        assert!(!v.systeme_fichiers.is_empty());
        assert!(v.total > 0);
    }
}
