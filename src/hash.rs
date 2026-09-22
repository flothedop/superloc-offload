//! Empreintes (checksums) des fichiers.
//!
//! Deux algorithmes : xxHash64 (rapide, standard du secteur) et MD5 (exigé
//! par certaines post-productions). Les deux peuvent être calculés en un seul
//! passage de lecture.

use md5::{Digest, Md5};
use serde::{Deserialize, Serialize};
use std::fmt;
use xxhash_rust::xxh64::Xxh64;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
pub enum Algo {
    Xxh64,
    Md5,
}

impl Algo {
    pub fn as_str(self) -> &'static str {
        match self {
            Algo::Xxh64 => "xxhash64",
            Algo::Md5 => "md5",
        }
    }

    /// Nom de l'élément XML correspondant dans un fichier MHL v1.1.
    pub fn mhl_element(self) -> &'static str {
        match self {
            // Représentation big-endian de l'empreinte 64 bits : c'est ce
            // qu'écrivent ShotPut Pro, Silverstack et consorts.
            Algo::Xxh64 => "xxhash64be",
            Algo::Md5 => "md5",
        }
    }

    pub fn parse(s: &str) -> Option<Algo> {
        match s.trim().to_ascii_lowercase().as_str() {
            "xxh64" | "xxhash64" | "xxhash64be" => Some(Algo::Xxh64),
            "md5" => Some(Algo::Md5),
            _ => None,
        }
    }
}

impl fmt::Display for Algo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

enum Single {
    Xxh64(Box<Xxh64>),
    Md5(Box<Md5>),
}

impl Single {
    fn new(algo: Algo) -> Self {
        match algo {
            Algo::Xxh64 => Single::Xxh64(Box::new(Xxh64::new(0))),
            Algo::Md5 => Single::Md5(Box::new(Md5::new())),
        }
    }

    fn update(&mut self, buf: &[u8]) {
        match self {
            Single::Xxh64(h) => h.update(buf),
            Single::Md5(h) => h.update(buf),
        }
    }

    fn finish_hex(self) -> String {
        match self {
            // {:016x} d'un u64 = représentation big-endian, conforme à la
            // forme canonique de xxHash et à l'élément <xxhash64be> du MHL.
            Single::Xxh64(h) => format!("{:016x}", h.digest()),
            Single::Md5(h) => format!("{:x}", h.finalize()),
        }
    }
}

/// Calcule plusieurs empreintes en un seul passage sur les données.
pub struct MultiHasher {
    algos: Vec<Algo>,
    state: Vec<Single>,
    bytes: u64,
}

impl MultiHasher {
    pub fn new(algos: &[Algo]) -> Self {
        let mut algos: Vec<Algo> = algos.to_vec();
        algos.sort();
        algos.dedup();
        let state = algos.iter().map(|a| Single::new(*a)).collect();
        MultiHasher {
            algos,
            state,
            bytes: 0,
        }
    }

    pub fn update(&mut self, buf: &[u8]) {
        self.bytes += buf.len() as u64;
        for s in self.state.iter_mut() {
            s.update(buf);
        }
    }

    pub fn bytes(&self) -> u64 {
        self.bytes
    }

    /// Renvoie les empreintes sous forme (algorithme, hexadécimal), triées.
    pub fn finish(self) -> Vec<(Algo, String)> {
        self.algos
            .into_iter()
            .zip(self.state.into_iter().map(|s| s.finish_hex()))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Vecteurs de référence de xxHash : XXH64("", seed=0) = 0xEF46DB3751D8E999
    #[test]
    fn xxh64_vecteur_connu_chaine_vide() {
        let h = MultiHasher::new(&[Algo::Xxh64]);
        assert_eq!(h.finish()[0].1, "ef46db3751d8e999");
    }

    #[test]
    fn xxh64_vecteur_connu_nobody() {
        let mut h = MultiHasher::new(&[Algo::Xxh64]);
        h.update(b"Nobody inspects the spammish repetition");
        assert_eq!(h.finish()[0].1, "fbcea83c8a378bf1");
    }

    #[test]
    fn md5_vecteur_connu() {
        let mut h = MultiHasher::new(&[Algo::Md5]);
        h.update(b"abc");
        assert_eq!(h.finish()[0].1, "900150983cd24fb0d6963f7d28e17f72");
    }

    #[test]
    fn calcul_simultane_identique_au_calcul_separe() {
        let data = b"A001_C003_0921XY.R3D contenu de test";
        let mut multi = MultiHasher::new(&[Algo::Md5, Algo::Xxh64]);
        multi.update(&data[..10]);
        multi.update(&data[10..]);
        let multi = multi.finish();

        let mut x = MultiHasher::new(&[Algo::Xxh64]);
        x.update(data);
        let mut m = MultiHasher::new(&[Algo::Md5]);
        m.update(data);

        assert_eq!(
            multi.iter().find(|(a, _)| *a == Algo::Xxh64).unwrap().1,
            x.finish()[0].1
        );
        assert_eq!(
            multi.iter().find(|(a, _)| *a == Algo::Md5).unwrap().1,
            m.finish()[0].1
        );
    }

    #[test]
    fn un_bit_modifie_change_lempreinte() {
        let mut a = MultiHasher::new(&[Algo::Xxh64]);
        a.update(b"rush intact");
        let mut b = MultiHasher::new(&[Algo::Xxh64]);
        b.update(b"rush intbct");
        assert_ne!(a.finish()[0].1, b.finish()[0].1);
    }
}
