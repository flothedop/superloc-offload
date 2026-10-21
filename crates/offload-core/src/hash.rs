//! Empreintes.
//!
//! Encodages conformes à ASC MHL v2.0 : hexadécimal minuscule, graine 0,
//! ordre big-endian. `xxh3` est XXH3 sur 64 bits, `xxh128` XXH3 sur 128 bits,
//! `c4` est l'identifiant C4 (SHA-512 en base 58, préfixe « c4 »).

use serde::{Deserialize, Serialize};

/// Algorithmes disponibles, dans l'ordre imposé par le schéma ASC MHL.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Algo {
    C4,
    Md5,
    Sha1,
    Xxh128,
    Xxh3,
    Xxh64,
}

/// Choix par défaut : xxh128 (standard actuel, référence ASC) et xxh64
/// (lu par tous les outils, exigé par Netflix en MHL v1.1).
pub const PAR_DEFAUT: [Algo; 2] = [Algo::Xxh128, Algo::Xxh64];

impl Algo {
    pub const TOUS: [Algo; 6] = [
        Algo::Xxh128,
        Algo::Xxh64,
        Algo::Xxh3,
        Algo::Md5,
        Algo::Sha1,
        Algo::C4,
    ];

    /// Nom de l'élément XML dans ASC MHL.
    pub fn nom(self) -> &'static str {
        match self {
            Algo::C4 => "c4",
            Algo::Md5 => "md5",
            Algo::Sha1 => "sha1",
            Algo::Xxh128 => "xxh128",
            Algo::Xxh3 => "xxh3",
            Algo::Xxh64 => "xxh64",
        }
    }

    pub fn depuis_nom(nom: &str) -> Option<Algo> {
        match nom.trim().to_ascii_lowercase().as_str() {
            "c4" => Some(Algo::C4),
            "md5" => Some(Algo::Md5),
            "sha1" => Some(Algo::Sha1),
            "xxh128" | "xxhash128" => Some(Algo::Xxh128),
            "xxh3" | "xxh3_64" | "xxhash3" => Some(Algo::Xxh3),
            "xxh64" | "xxhash64" | "xxhash64be" | "xxhash" => Some(Algo::Xxh64),
            _ => None,
        }
    }

    /// Nom de l'élément dans un MHL v1.1, quand l'algorithme y existe.
    pub fn nom_mhl1(self) -> Option<&'static str> {
        match self {
            Algo::Md5 => Some("md5"),
            Algo::Sha1 => Some("sha1"),
            Algo::Xxh64 => Some("xxhash64be"),
            _ => None,
        }
    }

    /// Rang de préférence pour comparer une copie à la carte : le plus
    /// grand gagne. Les xxHash sont conçus pour cela et bien plus rapides.
    fn preference(self) -> u8 {
        match self {
            Algo::Xxh128 => 6,
            Algo::Xxh3 => 5,
            Algo::Xxh64 => 4,
            Algo::Sha1 => 3,
            Algo::Md5 => 2,
            Algo::C4 => 1,
        }
    }
}

/// Algorithme retenu pour comparer les copies parmi ceux demandés.
pub fn de_comparaison(algos: &[Algo]) -> Algo {
    algos
        .iter()
        .copied()
        .max_by_key(|a| a.preference())
        .unwrap_or(Algo::Xxh128)
}

/// Normalise une liste d'algorithmes : sans doublon, jamais vide.
pub fn normaliser(algos: &[Algo]) -> Vec<Algo> {
    let mut v: Vec<Algo> = algos.to_vec();
    v.sort();
    v.dedup();
    if v.is_empty() {
        v = PAR_DEFAUT.to_vec();
        v.sort();
    }
    v
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Empreinte {
    pub algo: Algo,
    pub valeur: String,
}

/// Calcul incrémental d'une empreinte.
pub enum Hacheur {
    C4(Box<sha2::Sha512>),
    Md5(md5::Md5),
    Sha1(sha1::Sha1),
    Xxh128(Box<xxhash_rust::xxh3::Xxh3>),
    Xxh3(Box<xxhash_rust::xxh3::Xxh3>),
    Xxh64(xxhash_rust::xxh64::Xxh64),
}

impl Hacheur {
    pub fn new(algo: Algo) -> Hacheur {
        use sha2::Digest;
        match algo {
            Algo::C4 => Hacheur::C4(Box::new(sha2::Sha512::new())),
            Algo::Md5 => Hacheur::Md5(md5::Md5::new()),
            Algo::Sha1 => Hacheur::Sha1(sha1::Sha1::new()),
            Algo::Xxh128 => Hacheur::Xxh128(Box::new(xxhash_rust::xxh3::Xxh3::new())),
            Algo::Xxh3 => Hacheur::Xxh3(Box::new(xxhash_rust::xxh3::Xxh3::new())),
            Algo::Xxh64 => Hacheur::Xxh64(xxhash_rust::xxh64::Xxh64::new(0)),
        }
    }

    pub fn update(&mut self, data: &[u8]) {
        use sha2::Digest;
        match self {
            Hacheur::C4(h) => h.update(data),
            Hacheur::Md5(h) => h.update(data),
            Hacheur::Sha1(h) => h.update(data),
            Hacheur::Xxh128(h) | Hacheur::Xxh3(h) => h.update(data),
            Hacheur::Xxh64(h) => h.update(data),
        }
    }

    pub fn finir(self) -> String {
        use sha2::Digest;
        match self {
            Hacheur::C4(h) => c4_depuis_sha512(&h.finalize()),
            Hacheur::Md5(h) => hex(&h.finalize()),
            Hacheur::Sha1(h) => hex(&h.finalize()),
            Hacheur::Xxh128(h) => format!("{:032x}", h.digest128()),
            Hacheur::Xxh3(h) => format!("{:016x}", h.digest()),
            Hacheur::Xxh64(h) => format!("{:016x}", h.digest()),
        }
    }
}

/// Plusieurs empreintes calculées en une passe.
pub struct Multi {
    hacheurs: Vec<(Algo, Hacheur)>,
    octets: u64,
}

impl Multi {
    pub fn new(algos: &[Algo]) -> Multi {
        Multi {
            hacheurs: algos.iter().map(|a| (*a, Hacheur::new(*a))).collect(),
            octets: 0,
        }
    }

    pub fn update(&mut self, data: &[u8]) {
        self.octets += data.len() as u64;
        for (_, h) in self.hacheurs.iter_mut() {
            h.update(data);
        }
    }

    pub fn octets(&self) -> u64 {
        self.octets
    }

    pub fn finir(self) -> Vec<Empreinte> {
        self.hacheurs
            .into_iter()
            .map(|(algo, h)| Empreinte {
                algo,
                valeur: h.finir(),
            })
            .collect()
    }
}

/// Empreinte d'un contenu en mémoire.
pub fn de_donnees(algo: Algo, data: &[u8]) -> String {
    let mut h = Hacheur::new(algo);
    h.update(data);
    h.finir()
}

pub fn hex(octets: &[u8]) -> String {
    use std::fmt::Write;
    let mut s = String::with_capacity(octets.len() * 2);
    for o in octets {
        let _ = write!(s, "{o:02x}");
    }
    s
}

pub fn depuis_hex(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok())
        .collect()
}

const ALPHABET_C4: &[u8; 58] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";

/// Identifiant C4 : condensat SHA-512 lu comme un entier big-endian, écrit
/// en base 58, complété à gauche par « 1 » jusqu'à 88 caractères, préfixé de
/// « c4 » (90 caractères au total).
pub fn c4_depuis_sha512(condensat: &[u8]) -> String {
    let mut nombre: Vec<u8> = condensat.to_vec();
    let mut chiffres: Vec<u8> = Vec::with_capacity(88);
    while nombre.iter().any(|o| *o != 0) {
        let mut reste: u32 = 0;
        for o in nombre.iter_mut() {
            let v = (reste << 8) | u32::from(*o);
            *o = (v / 58) as u8;
            reste = v % 58;
        }
        chiffres.push(ALPHABET_C4[reste as usize]);
    }
    while chiffres.len() < 88 {
        chiffres.push(b'1');
    }
    chiffres.reverse();
    let mut s = String::from("c4");
    s.push_str(std::str::from_utf8(&chiffres).unwrap_or_default());
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    // Vecteurs : bibliothèque xxHash de référence et RFC 1321 ; recoupés
    // avec l'outil de référence de l'ASC (ascmhl 1.2).
    #[test]
    fn vecteurs_chaine_vide() {
        assert_eq!(de_donnees(Algo::Xxh64, b""), "ef46db3751d8e999");
        assert_eq!(de_donnees(Algo::Xxh3, b""), "2d06800538d394c2");
        assert_eq!(
            de_donnees(Algo::Xxh128, b""),
            "99aa06d3014798d86001c324468d497f"
        );
        assert_eq!(
            de_donnees(Algo::Md5, b""),
            "d41d8cd98f00b204e9800998ecf8427e"
        );
        assert_eq!(
            de_donnees(Algo::Sha1, b""),
            "da39a3ee5e6b4b0d3255bfef95601890afd80709"
        );
    }

    #[test]
    fn vecteurs_abc() {
        assert_eq!(de_donnees(Algo::Xxh64, b"abc"), "44bc2cf5ad770999");
        assert_eq!(de_donnees(Algo::Xxh3, b"abc"), "78af5f94892f3950");
        assert_eq!(
            de_donnees(Algo::Xxh128, b"abc"),
            "06b05ab6733a618578af5f94892f3950"
        );
    }

    #[test]
    fn c4_chaine_vide() {
        // Identifiant C4 de la chaîne vide, publié par le projet C4 (SMPTE ST 2114).
        let c4 = de_donnees(Algo::C4, b"");
        assert_eq!(c4.len(), 90);
        assert_eq!(
            c4,
            "c459dsjfscH38cYeXXYogktxf4Cd9ibshE3BHUo6a58hBXmRQdZrAkZzsWcbWtDg5oQstpDuni4Hirj75GEmTc1sFT"
        );
    }

    #[test]
    fn la_comparaison_prefere_xxh128() {
        assert_eq!(de_comparaison(&[Algo::Md5, Algo::Xxh128]), Algo::Xxh128);
        assert_eq!(de_comparaison(&[Algo::Md5]), Algo::Md5);
    }

    #[test]
    fn hex_aller_retour() {
        assert_eq!(depuis_hex("00ff10").unwrap(), vec![0, 255, 16]);
        assert_eq!(hex(&[0, 255, 16]), "00ff10");
    }
}
