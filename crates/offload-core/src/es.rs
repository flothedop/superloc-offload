//! Entrées-sorties système : création, écriture physique, dates, cache.
//!
//! C'est ici que se joue la différence entre « le système a accepté les
//! données » et « les données sont sur le disque ». Chaque fonction rend
//! compte de ce qu'elle a réellement obtenu, plutôt que de supposer.

use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io;
use std::path::Path;
use std::time::{Duration, SystemTime};

/// Ce que la synchronisation a obtenu.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Durabilite {
    /// Le système a confirmé l'écriture sur le support, cache du disque
    /// compris quand il sait le faire (macOS `F_FULLFSYNC`, Windows
    /// `FlushFileBuffers`, Linux `fsync`).
    Complete,
    /// macOS : `F_FULLFSYNC` refusé par ce système de fichiers (exFAT,
    /// certains disques réseau) ; repli sur `fsync`, qui ne vide pas le
    /// cache interne du disque.
    Degradee,
}

/// Crée un fichier qui ne doit pas exister. Jamais d'écrasement.
pub fn creer_exclusif(chemin: &Path) -> io::Result<File> {
    let f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(chemin)?;
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::io::AsRawFd;
        // Écriture sans cache : les pages écrites ne restent pas dans le
        // cache unifié, que la relecture pourrait sinon resservir. Un refus
        // n'est pas bloquant : l'éviction et la mesure qui suivent font foi.
        // SÛRETÉ : descripteur valide, emprunté le temps de l'appel.
        unsafe { libc::fcntl(f.as_raw_fd(), libc::F_NOCACHE, 1) };
    }
    Ok(f)
}

/// Force l'écriture physique du fichier.
pub fn synchroniser(f: &File) -> io::Result<Durabilite> {
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::io::AsRawFd;
        let fd = f.as_raw_fd();
        // SÛRETÉ : descripteur valide.
        if unsafe { libc::fcntl(fd, libc::F_FULLFSYNC) } != -1 {
            return Ok(Durabilite::Complete);
        }
        // SÛRETÉ : idem.
        if unsafe { libc::fsync(fd) } == 0 {
            return Ok(Durabilite::Degradee);
        }
        Err(io::Error::last_os_error())
    }
    #[cfg(not(target_os = "macos"))]
    {
        f.sync_all()?;
        Ok(Durabilite::Complete)
    }
}

/// Applique la date de modification de la carte à la copie.
pub fn appliquer_date(f: &File, modifie: SystemTime) -> io::Result<()> {
    f.set_modified(modifie)
}

/// Applique une date de modification à un dossier.
pub fn appliquer_date_dossier(dossier: &Path, modifie: SystemTime) -> io::Result<()> {
    #[cfg(windows)]
    let f = {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_WRITE_ATTRIBUTES: u32 = 0x0100;
        const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
        std::fs::OpenOptions::new()
            .access_mode(FILE_WRITE_ATTRIBUTES)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
            .open(dossier)?
    };
    #[cfg(not(windows))]
    let f = File::open(dossier)?;
    f.set_modified(modifie)
}

/// Force l'écriture des entrées d'un dossier (renommages). Sans effet sous
/// Windows, où les métadonnées sont journalisées par NTFS.
pub fn synchroniser_dossier(dossier: &Path) {
    #[cfg(unix)]
    if let Ok(f) = File::open(dossier) {
        let _ = f.sync_all();
    }
    #[cfg(not(unix))]
    let _ = dossier;
}

/// Résidence d'un fichier dans le cache du système, mesurée.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Residence {
    /// Faux si le système ne permet pas de mesurer (Windows).
    pub mesuree: bool,
    pub pages_en_cache: u64,
    pub pages: u64,
}

impl Residence {
    pub fn hors_cache(&self) -> bool {
        self.mesuree && self.pages_en_cache == 0
    }
}

/// Chasse le fichier du cache du système, puis mesure ce qu'il en reste.
///
/// Après l'écriture physique, les pages du fichier sont « propres » : le
/// système peut les jeter sans perte. Si la mesure confirme qu'il n'en
/// reste aucune, toute lecture ultérieure vient forcément du support — même
/// si un autre programme (indexation) relit le fichier entre-temps, il le
/// relit depuis le disque.
pub fn evincer(chemin: &Path) -> io::Result<Residence> {
    #[cfg(unix)]
    {
        unix::evincer(chemin)
    }
    #[cfg(not(unix))]
    {
        let _ = std::fs::metadata(chemin)?;
        Ok(Residence {
            mesuree: false,
            pages_en_cache: 0,
            pages: 0,
        })
    }
}

/// Mesure seule, sans éviction.
pub fn residence(chemin: &Path) -> io::Result<Residence> {
    #[cfg(unix)]
    {
        unix::mesurer(chemin)
    }
    #[cfg(not(unix))]
    {
        let _ = std::fs::metadata(chemin)?;
        Ok(Residence {
            mesuree: false,
            pages_en_cache: 0,
            pages: 0,
        })
    }
}

#[cfg(unix)]
mod unix {
    use super::Residence;
    use std::fs::File;
    use std::io;
    use std::os::unix::io::AsRawFd;
    use std::path::Path;

    /// Projection en mémoire d'un fichier, sans jamais y accéder : la
    /// projection seule ne charge aucune page.
    struct Projection {
        adresse: *mut libc::c_void,
        longueur: usize,
    }

    impl Projection {
        fn new(f: &File, longueur: usize) -> io::Result<Projection> {
            // SÛRETÉ : projection en lecture seule d'un descripteur valide,
            // jamais déréférencée par ce module.
            let adresse = unsafe {
                libc::mmap(
                    std::ptr::null_mut(),
                    longueur,
                    libc::PROT_READ,
                    libc::MAP_SHARED,
                    f.as_raw_fd(),
                    0,
                )
            };
            if adresse == libc::MAP_FAILED {
                return Err(io::Error::last_os_error());
            }
            Ok(Projection { adresse, longueur })
        }

        fn pages_en_cache(&self) -> io::Result<(u64, u64)> {
            let page = taille_page();
            let pages = self.longueur.div_ceil(page);
            // Par tranches, pour borner la mémoire sur les très gros fichiers.
            const TRANCHE: usize = 256 * 1024;
            let mut vecteur = vec![0u8; TRANCHE.min(pages)];
            let mut en_cache = 0u64;
            let mut debut = 0usize;
            while debut < pages {
                let n = TRANCHE.min(pages - debut);
                let longueur = if debut + n == pages {
                    self.longueur - debut * page
                } else {
                    n * page
                };
                // SÛRETÉ : sous-plage de la projection, vecteur de n entrées.
                let r = unsafe {
                    libc::mincore(
                        (self.adresse as *mut u8).add(debut * page) as *mut libc::c_void,
                        longueur,
                        vecteur.as_mut_ptr() as *mut _,
                    )
                };
                if r != 0 {
                    return Err(io::Error::last_os_error());
                }
                en_cache += vecteur[..n].iter().filter(|v| **v & 1 == 1).count() as u64;
                debut += n;
            }
            Ok((en_cache, pages as u64))
        }

        #[cfg(target_os = "macos")]
        fn invalider(&self) -> io::Result<()> {
            // SÛRETÉ : plage exacte de la projection.
            let r = unsafe { libc::msync(self.adresse, self.longueur, libc::MS_INVALIDATE) };
            if r != 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        }
    }

    impl Drop for Projection {
        fn drop(&mut self) {
            // SÛRETÉ : projection créée par mmap avec cette longueur.
            unsafe { libc::munmap(self.adresse, self.longueur) };
        }
    }

    fn taille_page() -> usize {
        // SÛRETÉ : appel sans effet de bord.
        let p = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
        if p > 0 {
            p as usize
        } else {
            4096
        }
    }

    pub fn mesurer(chemin: &Path) -> io::Result<Residence> {
        let f = File::open(chemin)?;
        let longueur = f.metadata()?.len() as usize;
        if longueur == 0 {
            return Ok(Residence {
                mesuree: true,
                pages_en_cache: 0,
                pages: 0,
            });
        }
        let p = Projection::new(&f, longueur)?;
        let (en_cache, pages) = p.pages_en_cache()?;
        Ok(Residence {
            mesuree: true,
            pages_en_cache: en_cache,
            pages,
        })
    }

    pub fn evincer(chemin: &Path) -> io::Result<Residence> {
        let f = File::open(chemin)?;
        let longueur = f.metadata()?.len() as usize;
        if longueur == 0 {
            return Ok(Residence {
                mesuree: true,
                pages_en_cache: 0,
                pages: 0,
            });
        }
        #[cfg(target_os = "linux")]
        {
            // SÛRETÉ : descripteur valide.
            unsafe { libc::posix_fadvise(f.as_raw_fd(), 0, 0, libc::POSIX_FADV_DONTNEED) };
        }
        let p = Projection::new(&f, longueur)?;
        #[cfg(target_os = "macos")]
        {
            p.invalider()?;
        }
        let (en_cache, pages) = p.pages_en_cache()?;
        Ok(Residence {
            mesuree: true,
            pages_en_cache: en_cache,
            pages,
        })
    }
}

/// Renomme sans jamais remplacer un fichier existant : l'opération échoue
/// si la cible existe, de façon atomique (pas de « vérifier puis
/// renommer », qui laisse une fenêtre).
pub fn renommer_sans_ecraser(de: &Path, vers: &Path) -> io::Result<()> {
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::ffi::OsStrExt;
        let a = std::ffi::CString::new(de.as_os_str().as_bytes())?;
        let b = std::ffi::CString::new(vers.as_os_str().as_bytes())?;
        // SÛRETÉ : chaînes C valides, descripteurs AT_FDCWD.
        let r = unsafe {
            libc::renameat2(
                libc::AT_FDCWD,
                a.as_ptr(),
                libc::AT_FDCWD,
                b.as_ptr(),
                libc::RENAME_NOREPLACE,
            )
        };
        if r == 0 {
            return Ok(());
        }
        let e = io::Error::last_os_error();
        // Système de fichiers qui ne connaît pas RENAME_NOREPLACE (exFAT,
        // FUSE anciens) : repli contrôlé.
        if matches!(e.raw_os_error(), Some(libc::EINVAL) | Some(libc::ENOSYS)) {
            return renommer_si_absent(de, vers);
        }
        Err(e)
    }
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::ffi::OsStrExt;
        let a = std::ffi::CString::new(de.as_os_str().as_bytes())?;
        let b = std::ffi::CString::new(vers.as_os_str().as_bytes())?;
        // SÛRETÉ : chaînes C valides.
        if unsafe { libc::renamex_np(a.as_ptr(), b.as_ptr(), libc::RENAME_EXCL) } == 0 {
            return Ok(());
        }
        let e = io::Error::last_os_error();
        if matches!(e.raw_os_error(), Some(libc::ENOTSUP) | Some(libc::EINVAL)) {
            return renommer_si_absent(de, vers);
        }
        Err(e)
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::{MoveFileExW, MOVEFILE_WRITE_THROUGH};
        let large = |p: &Path| -> Vec<u16> {
            p.as_os_str()
                .encode_wide()
                .chain(std::iter::once(0))
                .collect()
        };
        let (a, b) = (large(de), large(vers));
        // Sans MOVEFILE_REPLACE_EXISTING : échoue si la cible existe.
        // SÛRETÉ : chaînes larges terminées par 0.
        if unsafe { MoveFileExW(a.as_ptr(), b.as_ptr(), MOVEFILE_WRITE_THROUGH) } != 0 {
            return Ok(());
        }
        Err(io::Error::last_os_error())
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        renommer_si_absent(de, vers)
    }
}

#[allow(dead_code)]
fn renommer_si_absent(de: &Path, vers: &Path) -> io::Result<()> {
    if vers.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "le fichier définitif existe déjà",
        ));
    }
    std::fs::rename(de, vers)
}

/// Sous Windows, un antivirus ou l'indexation ouvre souvent un fichier juste
/// après son écriture et le verrouille un instant : renommage et suppression
/// échouent alors avec « accès refusé » ou « fichier utilisé ». On réessaie
/// avec un délai croissant pendant environ 3 secondes.
pub fn avec_patience<T>(mut f: impl FnMut() -> io::Result<T>) -> io::Result<T> {
    let mut delai = Duration::from_millis(20);
    let mut essais = 0;
    loop {
        match f() {
            Err(e) if essais < 9 && verrou_passager(&e) => {
                essais += 1;
                std::thread::sleep(delai);
                delai = (delai * 2).min(Duration::from_millis(800));
            }
            autre => return autre,
        }
    }
}

fn verrou_passager(e: &io::Error) -> bool {
    #[cfg(windows)]
    {
        // 5 : accès refusé, 32 : violation de partage, 33 : verrou.
        matches!(e.raw_os_error(), Some(5) | Some(32) | Some(33))
    }
    #[cfg(not(windows))]
    {
        let _ = e;
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creation_exclusive_refuse_lecrasement() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("a");
        creer_exclusif(&p).unwrap();
        assert!(creer_exclusif(&p).is_err());
    }

    #[test]
    fn le_renommage_nn_ecrase_jamais() {
        let d = tempfile::tempdir().unwrap();
        let (a, b) = (d.path().join("a"), d.path().join("b"));
        std::fs::write(&a, b"nouveau").unwrap();
        std::fs::write(&b, b"existant").unwrap();
        assert!(renommer_sans_ecraser(&a, &b).is_err());
        assert_eq!(std::fs::read(&b).unwrap(), b"existant");
        std::fs::remove_file(&b).unwrap();
        renommer_sans_ecraser(&a, &b).unwrap();
        assert_eq!(std::fs::read(&b).unwrap(), b"nouveau");
    }

    #[test]
    fn la_date_de_modification_est_appliquee() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("a");
        let f = creer_exclusif(&p).unwrap();
        let date = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        appliquer_date(&f, date).unwrap();
        drop(f);
        assert_eq!(std::fs::metadata(&p).unwrap().modified().unwrap(), date);
        appliquer_date_dossier(d.path(), date).unwrap();
        assert_eq!(
            std::fs::metadata(d.path()).unwrap().modified().unwrap(),
            date
        );
    }

    /// Le cœur de la vérification : après notre écriture, synchronisation et
    /// éviction, plus aucune page du fichier ne doit rester en mémoire. Ce
    /// test MESURE la résidence ; il ne se contente pas de vérifier qu'un
    /// drapeau a été posé.
    #[test]
    #[cfg(unix)]
    fn apres_eviction_le_fichier_nest_plus_en_cache() {
        use std::io::Write;
        // Dans target/ : un vrai disque, alors que /tmp est souvent un tmpfs.
        let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/essais");
        std::fs::create_dir_all(&base).unwrap();
        let d = tempfile::tempdir_in(&base).unwrap();
        let p = d.path().join("rush.bin");
        let mut f = creer_exclusif(&p).unwrap();
        let donnees = vec![0x5au8; 32 * 1024 * 1024];
        f.write_all(&donnees).unwrap();
        synchroniser(&f).unwrap();
        drop(f);
        let r = evincer(&p).unwrap();
        assert!(r.mesuree);
        assert!(r.pages > 0);
        if r.pages_en_cache != 0 && tmpfs(d.path()) {
            // Un tmpfs n'a pas de support : ses pages ne peuvent pas être
            // chassées. Le cas est signalé, pas masqué.
            eprintln!("tmpfs : éviction impossible, test non concluant ici");
            return;
        }
        assert_eq!(
            r.pages_en_cache, 0,
            "{} pages sur {} encore en cache après éviction",
            r.pages_en_cache, r.pages
        );
    }

    #[cfg(unix)]
    fn tmpfs(p: &Path) -> bool {
        crate::volume::du_chemin(p)
            .map(|v| v.systeme_fichiers.to_ascii_lowercase().contains("tmpfs"))
            .unwrap_or(false)
    }
}
