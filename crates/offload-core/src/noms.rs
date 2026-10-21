//! Noms de fichiers : normalisation Unicode, casse, noms interdits.
//!
//! Un même nom accentué peut s'écrire de deux façons en Unicode (« é » en un
//! caractère, NFC, ou « e » + accent, NFD). APFS préserve les octets mais
//! compare sans tenir compte de la forme ; HFS+ stocke une forme décomposée ;
//! exFAT et NTFS ignorent la casse. Deux fichiers distincts sur la carte
//! peuvent donc entrer en collision sur la destination : on le détecte avant
//! d'écrire quoi que ce soit.
//!
//! Les noms sont écrits tels que lus sur la carte ; ils ne sont normalisés
//! que pour être COMPARÉS.

use unicode_normalization::UnicodeNormalization;

pub fn nfc(s: &str) -> String {
    s.nfc().collect()
}

/// Clé de comparaison sur un volume insensible à la casse et à la forme.
pub fn cle_insensible(s: &str) -> String {
    nfc(s).to_lowercase().nfc().collect()
}

/// Systèmes de fichiers sensibles à la casse (les autres sont traités comme
/// insensibles, ce qui est l'hypothèse prudente).
pub fn sensible_a_la_casse(systeme: &str) -> bool {
    let s = systeme.to_ascii_lowercase();
    [
        "ext2", "ext3", "ext4", "btrfs", "xfs", "zfs", "f2fs", "tmpfs", "overlay",
    ]
    .iter()
    .any(|f| s.contains(f))
}

/// Volumes qui appliquent les règles de nommage de Windows.
pub fn regles_windows(systeme: &str) -> bool {
    if cfg!(windows) {
        return true;
    }
    let s = systeme.to_ascii_lowercase();
    ["ntfs", "fat", "msdos", "vfat", "exfat", "fuseblk"]
        .iter()
        .any(|f| s.contains(f))
}

const RESERVES: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "COM¹", "COM²", "COM³", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8",
    "LPT9", "LPT¹", "LPT²", "LPT³",
];

/// Raison pour laquelle Windows (NTFS, FAT, exFAT) refuserait ce composant
/// de chemin, ou `None` s'il est accepté.
pub fn refus_windows(composant: &str) -> Option<String> {
    if let Some(c) = composant
        .chars()
        .find(|c| matches!(c, '<' | '>' | ':' | '"' | '\\' | '|' | '?' | '*') || (*c as u32) < 32)
    {
        return Some(if (c as u32) < 32 {
            format!("caractère de contrôle U+{:04X}", c as u32)
        } else {
            format!("caractère « {c} »")
        });
    }
    if composant.ends_with('.') || composant.ends_with(' ') {
        return Some("point ou espace final".into());
    }
    let base = composant.split('.').next().unwrap_or("").trim_end();
    if RESERVES.iter().any(|r| r.eq_ignore_ascii_case(base)) {
        return Some(format!("nom réservé « {base} »"));
    }
    None
}

/// Paires de chemins qui désigneraient le même fichier sur la destination.
pub fn collisions<'a>(
    chemins: impl Iterator<Item = &'a str>,
    casse: bool,
) -> Vec<(String, String)> {
    let mut vus: std::collections::HashMap<String, &str> = std::collections::HashMap::new();
    let mut paires = Vec::new();
    for c in chemins {
        let cle = if casse { nfc(c) } else { cle_insensible(c) };
        match vus.get(&cle) {
            Some(premier) if *premier != c => paires.push((premier.to_string(), c.to_string())),
            Some(_) => {}
            None => {
                vus.insert(cle, c);
            }
        }
    }
    paires
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn les_formes_unicode_sont_comparees_a_egalite() {
        let compose = "Sc\u{e9}ne"; // é en un caractère
        let decompose = "Sce\u{301}ne"; // e + accent aigu
        assert_ne!(compose, decompose);
        assert_eq!(nfc(compose), nfc(decompose));
        let c = collisions([compose, decompose].into_iter(), true);
        assert_eq!(c.len(), 1, "collision même sur volume sensible à la casse");
    }

    #[test]
    fn la_casse_ne_compte_que_sur_un_volume_insensible() {
        let v = ["A001/clip.mov", "A001/CLIP.MOV"];
        assert_eq!(collisions(v.into_iter(), false).len(), 1);
        assert!(collisions(v.into_iter(), true).is_empty());
    }

    #[test]
    fn noms_refuses_par_windows() {
        assert!(refus_windows("CON").is_some());
        assert!(refus_windows("con.txt").is_some());
        assert!(refus_windows("COM1.R3D").is_some());
        assert!(refus_windows("plan:1.mov").is_some());
        assert!(refus_windows("fin.").is_some());
        assert!(refus_windows("fin ").is_some());
        assert!(refus_windows("a\u{1}b").is_some());
        assert!(refus_windows("A001_C003_0921XY_001.R3D").is_none());
        assert!(refus_windows("CONSOLE.txt").is_none());
    }

    #[test]
    fn systemes_de_fichiers() {
        assert!(sensible_a_la_casse("ext4"));
        assert!(!sensible_a_la_casse("apfs"));
        assert!(!sensible_a_la_casse("exfat"));
        assert!(regles_windows("exfat"));
    }
}
