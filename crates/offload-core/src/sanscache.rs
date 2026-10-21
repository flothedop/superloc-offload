//! Relecture qui contourne le cache du système.
//!
//! Juste après une copie, le système garde les données en mémoire vive : une
//! relecture ordinaire relirait la RAM et validerait une écriture pourtant
//! corrompue sur le disque.
//!
//! - Linux : `O_DIRECT`. Le noyau écrit puis invalide les pages en cache
//!   avant toute lecture directe : la lecture vient du support.
//! - Windows : `FILE_FLAG_NO_BUFFERING`, lecture non mise en cache.
//! - macOS : `F_NOCACHE` ne suffit PAS. Le noyau (XNU, `cluster_read_direct`)
//!   sert d'abord les pages déjà présentes en cache. La preuve vient donc de
//!   la mesure faite avant la relecture (`es::evincer`) : si aucune page du
//!   fichier n'est en cache, la lecture vient forcément du support.
//!
//! Le mode obtenu est consigné ; c'est le moteur qui en déduit le niveau de
//! vérification, en le combinant avec la mesure de résidence.

use crate::tampon::TamponAligne;
use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
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
    /// Vrai si le système garantit, à lui seul, que la lecture vient du
    /// support. Sinon, seule la mesure de résidence en cache peut le prouver.
    pub fn garantit_le_support(self) -> bool {
        matches!(self, Mode::Direct | Mode::NoBuffering)
    }
}

/// Ouvre un fichier pour le relire sans cache, sur toute sa taille actuelle.
pub fn ouvrir(chemin: &Path, taille_bloc: usize) -> io::Result<LecteurSansCache> {
    let taille = std::fs::metadata(chemin)?.len();
    ouvrir_interne(chemin, taille_bloc, taille)
}

/// Idem, en lisant `taille` octets : sert à relire une carte dont le
/// système annonce une taille fausse (volumes Codex HDE, qui affichent 0).
pub fn ouvrir_taille(
    chemin: &Path,
    taille_bloc: usize,
    taille: u64,
) -> io::Result<LecteurSansCache> {
    ouvrir_interne(chemin, taille_bloc, taille)
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
        let n = self.fichier.read(self.tampon.zone())?;
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
        Ok(&self.tampon.zone()[..utile])
    }
}

#[cfg(target_os = "linux")]
fn ouvrir_interne(chemin: &Path, taille_bloc: usize, taille: u64) -> io::Result<LecteurSansCache> {
    use std::os::unix::fs::OpenOptionsExt;

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
fn ouvrir_interne(chemin: &Path, taille_bloc: usize, taille: u64) -> io::Result<LecteurSansCache> {
    use std::os::unix::io::AsRawFd;

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
fn ouvrir_interne(chemin: &Path, taille_bloc: usize, taille: u64) -> io::Result<LecteurSansCache> {
    use std::os::windows::fs::OpenOptionsExt;

    const FILE_FLAG_NO_BUFFERING: u32 = 0x2000_0000;
    const FILE_FLAG_SEQUENTIAL_SCAN: u32 = 0x0800_0000;

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
fn ouvrir_interne(chemin: &Path, taille_bloc: usize, taille: u64) -> io::Result<LecteurSansCache> {
    Ok(LecteurSansCache {
        fichier: File::open(chemin)?,
        tampon: TamponAligne::new(taille_bloc)?,
        mode: Mode::Bufferise,
        taille,
        lus: 0,
    })
}
