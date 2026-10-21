//! Volumes : système de fichiers, espace, nom, disque physique.
//!
//! Ne sont retenues que les informations fiables. Le type de support (SSD ou
//! disque mécanique) est délibérément écarté : les bibliothèques le
//! rapportent faux dès qu'il y a une couche de virtualisation, et une
//! information fausse dans un diagnostic est pire que pas d'information.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use sysinfo::Disks;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Volume {
    pub point_montage: String,
    /// Nom du volume (étiquette), tel que l'affiche le système.
    pub nom: String,
    pub systeme_fichiers: String,
    pub amovible: bool,
    pub reseau: bool,
    pub total: u64,
    pub libre: u64,
    /// Identifiant du volume pendant cette session (périphérique Unix,
    /// numéro de série Windows).
    pub id_volume: Option<String>,
    /// Disque physique qui porte le volume, quand le système le dit. Deux
    /// volumes du même disque physique ne valent pas deux copies.
    pub id_disque: Option<String>,
}

impl Volume {
    /// Systèmes de fichiers coûteux en métadonnées avec beaucoup de petits
    /// fichiers.
    pub fn couteux_en_petits_fichiers(&self) -> bool {
        let f = self.systeme_fichiers.to_ascii_lowercase();
        f.contains("exfat") || f.contains("fat32") || f.contains("vfat") || f.contains("msdos")
    }

    /// FAT16/FAT32 : fichiers limités à 4 Gio moins un octet.
    pub fn limite_4_gio(&self) -> bool {
        let f = self.systeme_fichiers.to_ascii_lowercase();
        (f.contains("fat") && !f.contains("exfat")) || f == "msdos"
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
    // Un chemin « verbatim » local (\\?\C:\...) n'est pas réseau.
    let s = chemin.to_string_lossy();
    if let Some(reste) = s.strip_prefix("\\\\?\\") {
        return reste.to_ascii_uppercase().starts_with("UNC\\");
    }
    s.starts_with("\\\\") || s.starts_with("//")
}

fn nom_du_volume(point: &Path, nom_systeme: &str) -> String {
    if cfg!(windows) {
        if !nom_systeme.trim().is_empty() {
            return nom_systeme.trim().to_string();
        }
        return point.display().to_string();
    }
    point
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| point.display().to_string())
}

fn depuis_disque(d: &sysinfo::Disk) -> Volume {
    let point: PathBuf = d.mount_point().to_path_buf();
    let systeme = d.file_system().to_string_lossy().to_string();
    let ident = identite(&point);
    Volume {
        nom: nom_du_volume(&point, &d.name().to_string_lossy()),
        point_montage: point.display().to_string(),
        reseau: est_reseau(&systeme, &point) || imp::distant(&point),
        systeme_fichiers: systeme,
        amovible: d.is_removable(),
        total: d.total_space(),
        libre: d.available_space(),
        id_volume: ident.0,
        id_disque: ident.1,
    }
}

/// Volume qui porte `chemin` : le point de montage le plus spécifique.
pub fn du_chemin(chemin: &Path) -> Option<Volume> {
    // dunce : sous Windows, canonicalize() renvoie \\?\D:\... qui ne commence
    // jamais par un point de montage ordinaire (D:\).
    let absolu = dunce::canonicalize(chemin).unwrap_or_else(|_| chemin.to_path_buf());
    let disques = Disks::new_with_refreshed_list();
    let d = disques
        .list()
        .iter()
        .filter(|d| absolu.starts_with(d.mount_point()))
        .max_by_key(|d| d.mount_point().components().count())?;
    let mut v = depuis_disque(d);
    // L'identité se lit sur le chemin lui-même (un dossier peut être un
    // point de montage imbriqué que sysinfo ignore).
    let ident = identite(&absolu);
    v.id_volume = ident.0.or(v.id_volume);
    v.id_disque = ident.1.or(v.id_disque);
    v.reseau |= imp::distant(&absolu);
    Some(v)
}

/// Tous les volumes montés, amovibles d'abord.
pub fn lister() -> Vec<Volume> {
    let disques = Disks::new_with_refreshed_list();
    let mut v: Vec<Volume> = disques
        .list()
        .iter()
        .filter(|d| {
            let p = d.mount_point().to_string_lossy();
            // Volumes système de macOS, sans intérêt ici.
            !(p.starts_with("/System/Volumes") || p.starts_with("/private/var/vm"))
        })
        .map(depuis_disque)
        .collect();
    v.sort_by(|a, b| {
        b.amovible
            .cmp(&a.amovible)
            .then(a.point_montage.cmp(&b.point_montage))
    });
    v.dedup_by(|a, b| a.point_montage == b.point_montage);
    v
}

/// (identifiant du volume, identifiant du disque physique).
pub fn identite(chemin: &Path) -> (Option<String>, Option<String>) {
    imp::identite(chemin)
}

/// Vrai si deux identifiants de disque désignent au moins un disque
/// physique commun. Un identifiant peut en lister plusieurs (« sda,sdb »
/// pour un volume LVM ou RAID).
pub fn disques_communs(a: &str, b: &str) -> bool {
    let x: Vec<&str> = a.split(',').collect();
    b.split(',').any(|d| x.contains(&d))
}

#[cfg_attr(windows, allow(dead_code))]
fn joindre_disques(mut v: Vec<String>) -> Option<String> {
    v.sort();
    v.dedup();
    (!v.is_empty()).then(|| v.join(","))
}

#[cfg(target_os = "linux")]
mod imp {
    use std::os::unix::fs::MetadataExt;
    use std::path::{Path, PathBuf};

    const VIRTUEL: &str = "\u{0}virtuel";

    /// Disques entiers sous un périphérique bloc : une partition renvoie à
    /// son disque ; un volume dm (LVM, LUKS) ou md (RAID) renvoie aux disques
    /// de ses « esclaves ».
    fn disques_sous(p: &Path, profondeur: u8, sortie: &mut Vec<String>) {
        if profondeur > 8 {
            return;
        }
        let esclaves: Vec<PathBuf> = std::fs::read_dir(p.join("slaves"))
            .map(|it| it.filter_map(|e| e.ok()).map(|e| e.path()).collect())
            .unwrap_or_default();
        if !esclaves.is_empty() {
            for e in esclaves {
                if let Ok(c) = std::fs::canonicalize(&e) {
                    disques_sous(&c, profondeur + 1, sortie);
                }
            }
            return;
        }
        let disque = if p.join("partition").exists() {
            p.parent().and_then(|x| x.file_name())
        } else {
            p.file_name()
        };
        if let Some(n) = disque {
            sortie.push(n.to_string_lossy().to_string());
        }
        // Périphérique virtuel (image montée, disque en mémoire, réseau) :
        // marqué pour rendre l'ensemble non identifié.
        if p.components().any(|c| c.as_os_str() == "virtual") {
            sortie.push(VIRTUEL.into());
        }
    }

    pub fn identite(chemin: &Path) -> (Option<String>, Option<String>) {
        let Ok(m) = std::fs::metadata(chemin) else {
            return (None, None);
        };
        let dev = m.dev();
        let (maj, min) = (libc::major(dev), libc::minor(dev));
        let volume = Some(format!("{maj}:{min}"));
        // Majeur 0 : périphérique anonyme (btrfs, tmpfs, overlay, FUSE) —
        // impossible de remonter au disque physique.
        if maj == 0 {
            return (volume, None);
        }
        let Ok(p) = std::fs::canonicalize(format!("/sys/dev/block/{maj}:{min}")) else {
            return (volume, None);
        };
        let mut disques = Vec::new();
        disques_sous(&p, 0, &mut disques);
        if disques.iter().any(|d| d == VIRTUEL) {
            return (volume, None);
        }
        (volume, super::joindre_disques(disques))
    }

    pub fn distant(_: &Path) -> bool {
        false
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use crate::xml;
    use std::ffi::CStr;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::MetadataExt;
    use std::path::Path;

    pub fn identite(chemin: &Path) -> (Option<String>, Option<String>) {
        let volume = std::fs::metadata(chemin)
            .ok()
            .map(|m| format!("{}", m.dev()));
        let Ok(c) = std::ffi::CString::new(chemin.as_os_str().as_bytes()) else {
            return (volume, None);
        };
        // SÛRETÉ : statfs remplit la structure fournie ; chaîne terminée.
        let mut s: libc::statfs = unsafe { std::mem::zeroed() };
        if unsafe { libc::statfs(c.as_ptr(), &mut s) } != 0 {
            return (volume, None);
        }
        // SÛRETÉ : f_mntfromname est une chaîne C terminée par le noyau.
        let source = unsafe { CStr::from_ptr(s.f_mntfromname.as_ptr()) }
            .to_string_lossy()
            .to_string();
        let Some(noeud) = source.strip_prefix("/dev/") else {
            return (volume, None);
        };
        (volume, disques_physiques(noeud))
    }

    /// Disques physiques qui portent un volume, d'après `diskutil` : pour
    /// APFS, les « magasins physiques » du conteneur (un conteneur APFS est
    /// un disque synthétique, pas un disque physique) ; sinon le disque
    /// parent.
    fn diskutil(noeud: &str) -> Option<String> {
        let sortie = std::process::Command::new("/usr/sbin/diskutil")
            .args(["info", "-plist", noeud])
            .output()
            .ok()?;
        sortie
            .status
            .success()
            .then(|| String::from_utf8_lossy(&sortie.stdout).to_string())
    }

    fn disques_physiques(noeud: &str) -> Option<String> {
        let ids = analyser_plist(&diskutil(noeud)?)?;
        // Chaque disque retenu doit être physique : une image disque (.dmg,
        // .sparsebundle) montée depuis le disque interne n'est pas un
        // second disque.
        for d in ids.split(',') {
            if !est_physique(&diskutil(d)?) {
                return None;
            }
        }
        Some(ids)
    }

    pub fn est_physique(plist: &str) -> bool {
        let Ok(racine) = xml::lire(plist) else {
            return false;
        };
        let Some(dict) = racine.enfant("dict") else {
            return false;
        };
        let valeur = |cle: &str| -> Option<String> {
            let i = dict
                .enfants
                .iter()
                .position(|e| e.nom == "key" && e.texte.trim() == cle)?;
            dict.enfants.get(i + 1).map(|v| v.texte.trim().to_string())
        };
        valeur("VirtualOrPhysical").as_deref() == Some("Physical")
            && valeur("BusProtocol").as_deref() != Some("Disk Image")
    }

    pub fn analyser_plist(texte: &str) -> Option<String> {
        let racine = xml::lire(texte).ok()?;
        let dict = racine.enfant("dict")?;
        let valeur = |cle: &str| -> Option<&xml::Noeud> {
            let i = dict
                .enfants
                .iter()
                .position(|e| e.nom == "key" && e.texte.trim() == cle)?;
            dict.enfants.get(i + 1)
        };
        let mut disques = Vec::new();
        if let Some(tableau) = valeur("APFSPhysicalStores") {
            for d in tableau.enfants("dict") {
                for (i, e) in d.enfants.iter().enumerate() {
                    if e.nom == "key" && e.texte.trim() == "APFSPhysicalStore" {
                        if let Some(x) = d
                            .enfants
                            .get(i + 1)
                            .and_then(|v| disque_entier(v.texte.trim()))
                        {
                            disques.push(x);
                        }
                    }
                }
            }
        }
        if disques.is_empty() {
            if let Some(p) = valeur("ParentWholeDisk").and_then(|v| disque_entier(v.texte.trim())) {
                disques.push(p);
            }
        }
        super::joindre_disques(disques)
    }

    /// `disk4s2` ou `/dev/disk4s2` → `disk4`.
    pub fn disque_entier(source: &str) -> Option<String> {
        let nom = source.strip_prefix("/dev/").unwrap_or(source);
        let nom = nom.strip_prefix('r').unwrap_or(nom);
        let reste = nom.strip_prefix("disk")?;
        let chiffres: String = reste.chars().take_while(|c| c.is_ascii_digit()).collect();
        if chiffres.is_empty() {
            return None;
        }
        Some(format!("disk{chiffres}"))
    }

    pub fn distant(_: &Path) -> bool {
        false
    }
}

#[cfg(windows)]
mod imp {
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use std::path::Path;
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, GetDriveTypeW, GetVolumeInformationW, GetVolumePathNameW, FILE_SHARE_READ,
        FILE_SHARE_WRITE, OPEN_EXISTING,
    };
    use windows_sys::Win32::System::Ioctl::{
        PropertyStandardQuery, StorageDeviceProperty, IOCTL_STORAGE_GET_DEVICE_NUMBER,
        IOCTL_STORAGE_QUERY_PROPERTY, STORAGE_DEVICE_DESCRIPTOR, STORAGE_DEVICE_NUMBER,
        STORAGE_PROPERTY_QUERY,
    };
    use windows_sys::Win32::System::IO::DeviceIoControl;

    fn large(s: &std::ffi::OsStr) -> Vec<u16> {
        s.encode_wide().chain(std::iter::once(0)).collect()
    }

    /// Vrai si le périphérique est virtuel : disque virtuel (14), fichier
    /// image (15), espaces de stockage (16). En cas de doute : virtuel.
    fn bus_virtuel(h: windows_sys::Win32::Foundation::HANDLE) -> bool {
        let requete = STORAGE_PROPERTY_QUERY {
            PropertyId: StorageDeviceProperty,
            QueryType: PropertyStandardQuery,
            AdditionalParameters: [0],
        };
        let mut tampon = [0u64; 128];
        let mut rendu: u32 = 0;
        // SÛRETÉ : requête et tampon de sortie dimensionnés.
        let ok = unsafe {
            DeviceIoControl(
                h,
                IOCTL_STORAGE_QUERY_PROPERTY,
                &requete as *const _ as *const _,
                std::mem::size_of::<STORAGE_PROPERTY_QUERY>() as u32,
                tampon.as_mut_ptr() as *mut _,
                std::mem::size_of_val(&tampon) as u32,
                &mut rendu,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 || (rendu as usize) < std::mem::size_of::<STORAGE_DEVICE_DESCRIPTOR>() {
            return true;
        }
        // SÛRETÉ : le tampon (aligné sur 8) contient un descripteur complet.
        let d = unsafe { &*(tampon.as_ptr() as *const STORAGE_DEVICE_DESCRIPTOR) };
        matches!(d.BusType, 14..=16)
    }

    /// Lecteur réseau monté sous une lettre (le système de fichiers y est
    /// souvent annoncé « NTFS »).
    pub fn distant(chemin: &Path) -> bool {
        const DRIVE_REMOTE: u32 = 4;
        let c = large(chemin.as_os_str());
        let mut racine = vec![0u16; 1024];
        // SÛRETÉ : tampons dimensionnés, chaînes terminées par 0.
        if unsafe { GetVolumePathNameW(c.as_ptr(), racine.as_mut_ptr(), racine.len() as u32) } == 0
        {
            return false;
        }
        unsafe { GetDriveTypeW(racine.as_ptr()) == DRIVE_REMOTE }
    }

    pub fn identite(chemin: &Path) -> (Option<String>, Option<String>) {
        let chemin = large(chemin.as_os_str());
        let mut racine = vec![0u16; 1024];
        // SÛRETÉ : tampons dimensionnés, chaîne terminée par 0.
        if unsafe { GetVolumePathNameW(chemin.as_ptr(), racine.as_mut_ptr(), racine.len() as u32) }
            == 0
        {
            return (None, None);
        }
        let fin = racine.iter().position(|c| *c == 0).unwrap_or(racine.len());
        racine.truncate(fin);
        let mut racine_z = racine.clone();
        racine_z.push(0);

        let mut serie: u32 = 0;
        // SÛRETÉ : pointeurs nuls autorisés pour les tampons non voulus.
        let ok = unsafe {
            GetVolumeInformationW(
                racine_z.as_ptr(),
                std::ptr::null_mut(),
                0,
                &mut serie,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                0,
            )
        };
        let volume = (ok != 0).then(|| format!("{serie:08X}"));

        // « E:\ » → « \\.\E: » pour interroger le périphérique.
        let texte = std::ffi::OsString::from_wide(&racine)
            .to_string_lossy()
            .trim_end_matches('\\')
            .to_string();
        if texte.len() != 2 || !texte.ends_with(':') {
            return (volume, None);
        }
        let peripherique = large(std::ffi::OsStr::new(&format!("\\\\.\\{texte}")));
        // SÛRETÉ : ouverture sans droit d'accès, seulement pour l'IOCTL.
        let h = unsafe {
            CreateFileW(
                peripherique.as_ptr(),
                0,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                std::ptr::null(),
                OPEN_EXISTING,
                0,
                std::ptr::null_mut(),
            )
        };
        if h == INVALID_HANDLE_VALUE {
            return (volume, None);
        }
        let mut num: STORAGE_DEVICE_NUMBER = unsafe { std::mem::zeroed() };
        let mut rendu: u32 = 0;
        // SÛRETÉ : structure de sortie de la bonne taille.
        let ok = unsafe {
            DeviceIoControl(
                h,
                IOCTL_STORAGE_GET_DEVICE_NUMBER,
                std::ptr::null(),
                0,
                &mut num as *mut _ as *mut _,
                std::mem::size_of::<STORAGE_DEVICE_NUMBER>() as u32,
                &mut rendu,
                std::ptr::null_mut(),
            )
        };
        let virtuel = bus_virtuel(h);
        // SÛRETÉ : poignée valide.
        unsafe { CloseHandle(h) };
        // Disque virtuel (VHD, image), espace de stockage : ce n'est pas un
        // disque physique identifiable.
        let disque = (ok != 0 && !virtuel).then(|| format!("PhysicalDrive{}", num.DeviceNumber));
        (volume, disque)
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
mod imp {
    pub fn identite(_: &std::path::Path) -> (Option<String>, Option<String>) {
        (None, None)
    }
    pub fn distant(_: &std::path::Path) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reconnait_un_volume_reseau() {
        assert!(est_reseau("smbfs", Path::new("/Volumes/partage")));
        assert!(est_reseau("nfs", Path::new("/mnt/nas")));
        assert!(!est_reseau("apfs", Path::new("/Volumes/RAID")));
        assert!(est_reseau("ntfs", Path::new(r"\\serveur\rushs")));
        assert!(est_reseau("ntfs", Path::new(r"\\?\UNC\serveur\rushs")));
        assert!(!est_reseau("ntfs", Path::new(r"\\?\D:\rushs")));
    }

    #[test]
    fn limites_des_systemes_de_fichiers() {
        let v = |f: &str| Volume {
            systeme_fichiers: f.into(),
            ..Default::default()
        };
        assert!(v("exfat").couteux_en_petits_fichiers());
        assert!(!v("apfs").couteux_en_petits_fichiers());
        assert!(v("vfat").limite_4_gio());
        assert!(v("FAT32").limite_4_gio());
        assert!(v("msdos").limite_4_gio());
        assert!(!v("exfat").limite_4_gio());
        assert!(!v("ntfs").limite_4_gio());
    }

    #[test]
    fn trouve_le_volume_du_dossier_courant() {
        let v = du_chemin(Path::new(".")).expect("un volume doit porter le dossier courant");
        assert!(!v.systeme_fichiers.is_empty());
        assert!(v.total > 0);
        #[cfg(any(target_os = "linux", target_os = "macos", windows))]
        assert!(v.id_volume.is_some(), "identifiant de volume attendu");
    }

    #[test]
    fn disques_communs_entre_identifiants() {
        assert!(disques_communs("sda", "sda"));
        assert!(disques_communs("sda,sdb", "sdb"));
        assert!(!disques_communs("sda", "sdb"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn disque_entier_macos() {
        assert_eq!(imp::disque_entier("/dev/disk4s2").as_deref(), Some("disk4"));
        assert_eq!(imp::disque_entier("disk12").as_deref(), Some("disk12"));
        assert_eq!(imp::disque_entier("map auto_home"), None);
    }

    /// Un conteneur APFS (disk5) est synthétique : c'est son magasin
    /// physique (disk4s2, donc disk4) qui identifie le disque.
    #[cfg(target_os = "macos")]
    #[test]
    fn plist_diskutil_apfs() {
        let p = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>APFSContainerReference</key><string>disk5</string>
<key>APFSPhysicalStores</key><array><dict><key>APFSPhysicalStore</key><string>disk4s2</string></dict></array>
<key>ParentWholeDisk</key><string>disk5</string>
</dict></plist>"#;
        assert_eq!(imp::analyser_plist(p).as_deref(), Some("disk4"));
        let q = r#"<plist version="1.0"><dict><key>ParentWholeDisk</key><string>disk6</string></dict></plist>"#;
        assert_eq!(imp::analyser_plist(q).as_deref(), Some("disk6"));
        let image = r#"<plist version="1.0"><dict><key>BusProtocol</key><string>Disk Image</string><key>VirtualOrPhysical</key><string>Virtual</string></dict></plist>"#;
        assert!(!imp::est_physique(image));
        let ssd = r#"<plist version="1.0"><dict><key>BusProtocol</key><string>USB</string><key>VirtualOrPhysical</key><string>Physical</string></dict></plist>"#;
        assert!(imp::est_physique(ssd));
    }

    #[test]
    fn le_disque_du_dossier_courant_est_identifie_ou_absent() {
        // Sur un vrai disque, l'identifiant ne doit jamais être vide.
        let (_, d) = identite(Path::new("."));
        if let Some(d) = d {
            assert!(!d.is_empty());
        }
    }
}
