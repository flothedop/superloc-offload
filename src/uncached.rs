//! Lecture qui contourne le cache du système.
//!
//! C'est le point critique de la vérification. Juste après une copie, le
//! système d'exploitation garde les données en mémoire vive : une relecture
//! ordinaire relit la RAM, pas le disque, et validerait une écriture
//! pourtant corrompue. On force donc la lecture à descendre jusqu'au
//! support :
//!
//! - Linux   : `O_DIRECT` (repli : `posix_fadvise(DONTNEED)`)
//! - macOS   : `fcntl(F_NOCACHE)`
//! - Windows : `FILE_FLAG_NO_BUFFERING`
//!
//! Le mode réellement obtenu est remonté dans le rapport : si le système a
//! refusé le contournement, la vérification est dégradée et l'utilisateur
//! doit le savoir plutôt que de voir un voyant vert trompeur.

use std::alloc::{alloc, dealloc, Layout};
use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

/// Alignement exigé par les lectures sans cache (taille de secteur).
pub const ALIGN: usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Mode {
    /// Linux `O_DIRECT` : le cache est entièrement court-circuité.
    Direct,
    /// macOS `F_NOCACHE`.
    NoCache,
    /// Windows `FILE_FLAG_NO_BUFFERING`.
    NoBuffering,
    /// Linux, repli : les pages en cache sont invalidées avant lecture.
    FadviseDontNeed,
    /// Aucun contournement obtenu : vérification dégradée.
    Bufferise,
}

impl Mode {
    /// Vrai si la lecture a réellement atteint le support.
    pub fn fiable(self) -> bool {
        !matches!(self, Mode::Bufferise)
    }

    pub fn libelle(self) -> &'static str {
        match self {
            Mode::Direct => "lecture directe (O_DIRECT)",
            Mode::NoCache => "lecture sans cache (F_NOCACHE)",
            Mode::NoBuffering => "lecture sans tampon (NO_BUFFERING)",
            Mode::FadviseDontNeed => "cache invalidé avant lecture (fadvise)",
            Mode::Bufferise => "LECTURE MISE EN CACHE — vérification dégradée",
        }
    }
}

/// Tampon aligné en mémoire, nécessaire aux lectures directes.
struct TamponAligne {
    ptr: *mut u8,
    taille: usize,
}

// Le pointeur n'est manipulé que par le propriétaire du tampon.
unsafe impl Send for TamponAligne {}

impl TamponAligne {
    fn new(taille: usize) -> io::Result<Self> {
        let taille = taille.next_multiple_of(ALIGN);
        let layout = Layout::from_size_align(taille, ALIGN)
            .map_err(|e| io::Error::other(format!("tampon invalide : {e}")))?;
        // SÛRETÉ : layout a une taille non nulle ; le pointeur est vérifié.
        let ptr = unsafe { alloc(layout) };
        if ptr.is_null() {
            return Err(io::Error::other("allocation du tampon impossible"));
        }
        Ok(TamponAligne { ptr, taille })
    }

    fn as_mut_slice(&mut self) -> &mut [u8] {
        // SÛRETÉ : ptr est valide et alloué pour `taille` octets.
        unsafe { std::slice::from_raw_parts_mut(self.ptr, self.taille) }
    }
}

impl Drop for TamponAligne {
    fn drop(&mut self) {
        if !self.ptr.is_null() {
            let layout = Layout::from_size_align(self.taille, ALIGN).expect("layout déjà validé");
            // SÛRETÉ : même layout que lors de l'allocation.
            unsafe { dealloc(self.ptr, layout) }
        }
    }
}

/// Lecteur séquentiel qui ne passe pas par le cache du système.
pub struct LecteurSansCache {
    fichier: File,
    tampon: TamponAligne,
    mode: Mode,
    taille: u64,
    lus: u64,
}

impl LecteurSansCache {
    pub fn mode(&self) -> Mode {
        self.mode
    }

    pub fn taille(&self) -> u64 {
        self.taille
    }

    /// Lit le bloc suivant. Renvoie une tranche vide en fin de fichier.
    pub fn lire_bloc(&mut self) -> io::Result<&[u8]> {
        if self.lus >= self.taille {
            return Ok(&[]);
        }
        let n = self.fichier.read(self.tampon.as_mut_slice())?;
        if n == 0 {
            // Fin de fichier prématurée : le fichier est plus court qu'annoncé.
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                format!(
                    "fichier tronqué : {} octets lus sur {} attendus",
                    self.lus, self.taille
                ),
            ));
        }
        self.lus += n as u64;
        // La lecture peut déborder au-delà de la fin du fichier lorsque la
        // taille n'est pas un multiple de la taille de secteur : on ne rend
        // que les octets réellement utiles.
        let utile = if self.lus > self.taille {
            n - (self.lus - self.taille) as usize
        } else {
            n
        };
        Ok(&self.tampon.as_mut_slice()[..utile])
    }
}

#[cfg(target_os = "linux")]
pub fn ouvrir(chemin: &Path, taille_bloc: usize) -> io::Result<LecteurSansCache> {
    use std::os::unix::fs::OpenOptionsExt;

    let taille = std::fs::metadata(chemin)?.len();
    let tampon = TamponAligne::new(taille_bloc)?;

    // O_DIRECT n'est pas accepté par tous les systèmes de fichiers
    // (tmpfs, certains montages réseau) : on se replie proprement.
    match std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECT)
        .open(chemin)
    {
        Ok(fichier) => Ok(LecteurSansCache {
            fichier,
            tampon,
            mode: Mode::Direct,
            taille,
            lus: 0,
        }),
        Err(_) => {
            let fichier = File::open(chemin)?;
            let mode = invalider_cache(&fichier);
            Ok(LecteurSansCache {
                fichier,
                tampon,
                mode,
                taille,
                lus: 0,
            })
        }
    }
}

#[cfg(target_os = "linux")]
fn invalider_cache(fichier: &File) -> Mode {
    use std::os::unix::io::AsRawFd;
    // SÛRETÉ : descripteur valide, emprunté le temps de l'appel.
    let r = unsafe { libc::posix_fadvise(fichier.as_raw_fd(), 0, 0, libc::POSIX_FADV_DONTNEED) };
    if r == 0 {
        Mode::FadviseDontNeed
    } else {
        Mode::Bufferise
    }
}

#[cfg(target_os = "macos")]
pub fn ouvrir(chemin: &Path, taille_bloc: usize) -> io::Result<LecteurSansCache> {
    use std::os::unix::io::AsRawFd;

    let taille = std::fs::metadata(chemin)?.len();
    let tampon = TamponAligne::new(taille_bloc)?;
    let fichier = File::open(chemin)?;

    // F_NOCACHE : les lectures ne passent plus par le cache unifié et
    // n'exigent aucun alignement particulier.
    // SÛRETÉ : descripteur valide, emprunté le temps de l'appel.
    let r = unsafe { libc::fcntl(fichier.as_raw_fd(), libc::F_NOCACHE, 1) };
    let mode = if r == -1 {
        Mode::Bufferise
    } else {
        Mode::NoCache
    };

    // F_RDAHEAD désactive la lecture anticipée, qui pourrait resservir des
    // données déjà en mémoire.
    // SÛRETÉ : idem.
    unsafe { libc::fcntl(fichier.as_raw_fd(), libc::F_RDAHEAD, 0) };

    Ok(LecteurSansCache {
        fichier,
        tampon,
        mode,
        taille,
        lus: 0,
    })
}

#[cfg(target_os = "windows")]
pub fn ouvrir(chemin: &Path, taille_bloc: usize) -> io::Result<LecteurSansCache> {
    use std::os::windows::fs::OpenOptionsExt;

    const FILE_FLAG_NO_BUFFERING: u32 = 0x2000_0000;
    const FILE_FLAG_SEQUENTIAL_SCAN: u32 = 0x0800_0000;

    let taille = std::fs::metadata(chemin)?.len();
    let tampon = TamponAligne::new(taille_bloc)?;

    match std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_NO_BUFFERING | FILE_FLAG_SEQUENTIAL_SCAN)
        .open(chemin)
    {
        Ok(fichier) => Ok(LecteurSansCache {
            fichier,
            tampon,
            mode: Mode::NoBuffering,
            taille,
            lus: 0,
        }),
        Err(_) => {
            let fichier = std::fs::OpenOptions::new()
                .read(true)
                .custom_flags(FILE_FLAG_SEQUENTIAL_SCAN)
                .open(chemin)?;
            Ok(LecteurSansCache {
                fichier,
                tampon,
                mode: Mode::Bufferise,
                taille,
                lus: 0,
            })
        }
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
pub fn ouvrir(chemin: &Path, taille_bloc: usize) -> io::Result<LecteurSansCache> {
    let taille = std::fs::metadata(chemin)?.len();
    Ok(LecteurSansCache {
        fichier: File::open(chemin)?,
        tampon: TamponAligne::new(taille_bloc)?,
        mode: Mode::Bufferise,
        taille,
        lus: 0,
    })
}
