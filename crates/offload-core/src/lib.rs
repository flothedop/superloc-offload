//! Moteur de déchargement et de vérification de rushs.
//!
//! La carte n'est lue qu'une fois. Chaque bloc lu est partagé entre les fils
//! qui calculent les empreintes (un par algorithme) et les fils qui écrivent
//! sur chaque destination (un par disque). Chaque destination écrit, force
//! l'écriture physique, chasse le fichier du cache, mesure qu'il en est bien
//! sorti, le relit depuis le support et compare son empreinte à celle de la
//! carte. Le fichier ne reçoit son nom définitif qu'à ce moment-là.
//!
//! Le niveau de preuve obtenu est consigné fichier par fichier : l'outil
//! n'affirme jamais plus que ce qu'il a mesuré. Voir `docs/CONCEPTION.md`.

pub mod ascmhl;
pub mod controle;
pub mod diagnostic;
pub mod erreur;
pub mod es;
pub mod faute;
pub mod gabarit;
pub mod hash;
pub mod inventaire;
pub mod langue;
pub mod mesure;
pub mod mhl1;
pub mod moteur;
pub mod noms;
pub mod rapport;
pub mod rapport_html;
pub mod registre;
pub mod sanscache;
pub mod tampon;
pub mod verification;
pub mod volume;
pub mod xml;

/// Nom et version de l'outil, repris dans les manifestes et les rapports.
pub const OUTIL: &str = "SuperLoc Offload";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Taille des blocs de lecture et d'écriture. Multiple de 16 Kio (taille de
/// page des Mac Apple Silicon), donc aussi de 4 Kio (secteur), comme
/// l'exigent les lectures sans cache.
pub const BLOC: usize = 8 * 1024 * 1024;

/// Nombre de tampons en circulation. C'est la mémoire maximale consommée par
/// une copie (32 × 8 Mio = 256 Mio), quel que soit le nombre de disques : les
/// blocs sont partagés entre destinations, pas recopiés.
pub const TAMPONS: usize = 32;

/// Suffixe des fichiers en cours de copie.
pub const SUFFIXE_TEMP: &str = ".offload-part";

/// Dossier où l'outil range ses rapports sur chaque destination.
pub const DOSSIER_RAPPORT: &str = "_offload";
