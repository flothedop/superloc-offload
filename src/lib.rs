//! Moteur de déchargement et de vérification de rushs.
//!
//! Principe : la carte source n'est lue qu'une fois ; les données sont
//! écrites en parallèle vers toutes les destinations et l'empreinte est
//! calculée à la volée. Chaque destination est ensuite relue depuis le
//! disque, en contournant le cache système, et son empreinte est comparée à
//! celle de la source. Un fichier n'obtient son nom définitif qu'une fois
//! vérifié.

pub mod copy;
pub mod hash;
pub mod mhl;
pub mod preflight;
pub mod report;
pub mod scan;
pub mod uncached;
pub mod verify;

/// Taille des blocs de lecture/écriture. Multiple de 4 KiB, requis par les
/// lectures sans cache (O_DIRECT sous Linux, NO_BUFFERING sous Windows).
pub const CHUNK: usize = 4 * 1024 * 1024;
