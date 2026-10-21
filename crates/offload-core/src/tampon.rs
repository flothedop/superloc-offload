//! Tampons alignés et réserve recyclée.
//!
//! La réserve fixe la mémoire maximale d'une copie : la lecture de la carte
//! prend un tampon libre, le remplit et le partage (`Arc`) entre les fils
//! d'empreinte et les destinations. Quand le dernier l'a consommé, il
//! revient tout seul dans la réserve. S'il n'y a plus de tampon libre, la
//! lecture attend : c'est la contre-pression, et le temps attendu désigne le
//! maillon lent.

use crossbeam_channel::{unbounded, Receiver, RecvTimeoutError, Sender};
use std::alloc::{alloc, dealloc, Layout};
use std::io;
use std::ops::Deref;
use std::time::Duration;

/// Alignement des tampons : une page de Mac Apple Silicon (16 Kio), donc
/// aussi un multiple de la taille de secteur (512 o ou 4 Kio).
pub const ALIGNEMENT: usize = 16 * 1024;

/// Zone mémoire alignée, de capacité fixe.
pub struct TamponAligne {
    ptr: *mut u8,
    capacite: usize,
    longueur: usize,
}

// SÛRETÉ : la zone appartient en propre au tampon ; une fois partagée via
// `Arc<Bloc>`, elle n'est plus accessible qu'en lecture.
unsafe impl Send for TamponAligne {}
unsafe impl Sync for TamponAligne {}

impl TamponAligne {
    pub fn new(capacite: usize) -> io::Result<TamponAligne> {
        let capacite = capacite.max(ALIGNEMENT).next_multiple_of(ALIGNEMENT);
        let layout = Layout::from_size_align(capacite, ALIGNEMENT)
            .map_err(|e| io::Error::other(format!("tampon invalide : {e}")))?;
        // SÛRETÉ : taille non nulle ; le pointeur est vérifié ci-dessous.
        let ptr = unsafe { alloc(layout) };
        if ptr.is_null() {
            return Err(io::Error::other("mémoire insuffisante pour le tampon"));
        }
        Ok(TamponAligne {
            ptr,
            capacite,
            longueur: 0,
        })
    }

    pub fn capacite(&self) -> usize {
        self.capacite
    }

    /// Zone entière, pour la remplir.
    pub fn zone(&mut self) -> &mut [u8] {
        // SÛRETÉ : ptr est valide pour `capacite` octets et nous en avons
        // l'usage exclusif (&mut self).
        unsafe { std::slice::from_raw_parts_mut(self.ptr, self.capacite) }
    }

    pub fn fixer_longueur(&mut self, n: usize) {
        self.longueur = n.min(self.capacite);
    }

    pub fn donnees(&self) -> &[u8] {
        // SÛRETÉ : ptr est valide pour `capacite` ≥ `longueur` octets.
        unsafe { std::slice::from_raw_parts(self.ptr, self.longueur) }
    }

    /// Partie modifiable des données (sert à l'injection de pannes).
    pub fn donnees_mut(&mut self) -> &mut [u8] {
        let n = self.longueur;
        &mut self.zone()[..n]
    }
}

impl Drop for TamponAligne {
    fn drop(&mut self) {
        if let Ok(layout) = Layout::from_size_align(self.capacite, ALIGNEMENT) {
            // SÛRETÉ : même layout qu'à l'allocation.
            unsafe { dealloc(self.ptr, layout) }
        }
    }
}

/// Bloc rempli, partagé en lecture seule. Revient à la réserve à sa
/// destruction.
pub struct Bloc {
    tampon: Option<TamponAligne>,
    retour: Sender<TamponAligne>,
}

impl Deref for Bloc {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        self.tampon.as_ref().map(|t| t.donnees()).unwrap_or(&[])
    }
}

impl Drop for Bloc {
    fn drop(&mut self) {
        if let Some(mut t) = self.tampon.take() {
            t.fixer_longueur(0);
            // Si la réserve a disparu, le tampon est simplement libéré.
            let _ = self.retour.send(t);
        }
    }
}

pub struct Reserve {
    libres: Receiver<TamponAligne>,
    retour: Sender<TamponAligne>,
    nombre: usize,
}

impl Reserve {
    pub fn new(nombre: usize, taille: usize) -> io::Result<Reserve> {
        let (retour, libres) = unbounded();
        for _ in 0..nombre.max(2) {
            retour
                .send(TamponAligne::new(taille)?)
                .map_err(|_| io::Error::other("réserve fermée"))?;
        }
        Ok(Reserve {
            libres,
            retour,
            nombre: nombre.max(2),
        })
    }

    /// Tampon libre, ou `None` si aucun ne s'est libéré dans le délai.
    pub fn prendre(&self, delai: Duration) -> Option<TamponAligne> {
        match self.libres.recv_timeout(delai) {
            Ok(t) => Some(t),
            Err(RecvTimeoutError::Timeout) | Err(RecvTimeoutError::Disconnected) => None,
        }
    }

    pub fn emballer(&self, tampon: TamponAligne) -> Bloc {
        Bloc {
            tampon: Some(tampon),
            retour: self.retour.clone(),
        }
    }

    pub fn libres(&self) -> usize {
        self.libres.len()
    }

    pub fn nombre(&self) -> usize {
        self.nombre
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn un_bloc_revient_dans_la_reserve_quand_plus_personne_ne_le_tient() {
        let r = Reserve::new(2, 4096).unwrap();
        let mut t = r.prendre(Duration::from_millis(10)).unwrap();
        t.zone()[..3].copy_from_slice(b"abc");
        t.fixer_longueur(3);
        let bloc = Arc::new(r.emballer(t));
        let copie = Arc::clone(&bloc);
        assert_eq!(&bloc[..], b"abc");
        assert_eq!(r.libres(), 1);
        drop(bloc);
        assert_eq!(r.libres(), 1, "encore tenu par une destination");
        drop(copie);
        assert_eq!(r.libres(), 2);
    }

    #[test]
    fn la_reserve_epuisee_fait_attendre() {
        let r = Reserve::new(2, 4096).unwrap();
        let _a = r.prendre(Duration::from_millis(10)).unwrap();
        let _b = r.prendre(Duration::from_millis(10)).unwrap();
        assert!(r.prendre(Duration::from_millis(10)).is_none());
    }

    #[test]
    fn les_tampons_sont_alignes() {
        let mut t = TamponAligne::new(100).unwrap();
        assert_eq!(t.zone().as_ptr() as usize % ALIGNEMENT, 0);
        assert_eq!(t.capacite(), ALIGNEMENT);
    }
}
