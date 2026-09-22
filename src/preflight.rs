//! Contrôles préalables : rien ne démarre tant que la copie n'est pas sûre.

use crate::scan::Scan;
use fs4::available_space;
use std::io;
use std::path::{Path, PathBuf};

/// Marge de sécurité au-delà du volume des rushs (métadonnées, MHL, rapport).
const MARGE: u64 = 64 * 1024 * 1024;

#[derive(Debug)]
pub struct Preflight {
    pub destinations: Vec<PathBuf>,
    pub avertissements: Vec<String>,
}

/// Vérifie chaque destination avant la moindre écriture :
/// espace disponible, absence d'écrasement, droits d'écriture réels.
pub fn controler(scan: &Scan, destinations: &[PathBuf], source: &Path) -> io::Result<Preflight> {
    if destinations.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "aucune destination indiquée",
        ));
    }
    if scan.fichiers.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "aucun fichier à copier dans la source",
        ));
    }

    // dunce : sous Windows, évite les chemins « verbatim » (préfixe \\?\) dans
    // les messages et les comparaisons.
    let source_abs = dunce::canonicalize(source)?;
    let mut avertissements = Vec::new();
    let mut vues: Vec<PathBuf> = Vec::new();

    for dest in destinations {
        // Rien n'est jamais écrasé : le dossier cible ne doit pas exister.
        if dest.exists() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!(
                    "le dossier de destination existe déjà : {} — la copie est annulée pour ne rien écraser",
                    dest.display()
                ),
            ));
        }

        let parent = dest.parent().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("destination invalide : {}", dest.display()),
            )
        })?;
        let parent = if parent.as_os_str().is_empty() {
            Path::new(".")
        } else {
            parent
        };
        if !parent.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!(
                    "disque ou dossier de destination absent : {}",
                    parent.display()
                ),
            ));
        }

        let parent_abs = dunce::canonicalize(parent)?;

        // Copier dans la source elle-même créerait une récursion.
        if parent_abs.starts_with(&source_abs) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "la destination {} se trouve à l'intérieur de la source",
                    dest.display()
                ),
            ));
        }

        // Deux destinations sur le même dossier : la copie serait inutile.
        if vues.iter().any(|v| v == &parent_abs) {
            avertissements.push(format!(
                "deux destinations pointent vers le même emplacement : {}",
                parent_abs.display()
            ));
        }
        vues.push(parent_abs.clone());

        // Espace disponible.
        let besoin = scan.octets_total + MARGE;
        let dispo = available_space(&parent_abs)?;
        if dispo < besoin {
            return Err(io::Error::new(
                io::ErrorKind::StorageFull,
                format!(
                    "espace insuffisant sur {} : {} disponibles pour {} nécessaires",
                    parent_abs.display(),
                    octets_lisibles(dispo),
                    octets_lisibles(besoin)
                ),
            ));
        }

        // Droit d'écriture réel, vérifié en écrivant vraiment.
        let temoin = parent_abs.join(format!(".offload-test-{}", std::process::id()));
        std::fs::write(&temoin, b"test").map_err(|e| {
            io::Error::new(
                e.kind(),
                format!("écriture impossible sur {} : {e}", parent_abs.display()),
            )
        })?;
        let _ = std::fs::remove_file(&temoin);
    }

    Ok(Preflight {
        destinations: destinations.to_vec(),
        avertissements,
    })
}

pub fn octets_lisibles(n: u64) -> String {
    const U: [&str; 5] = ["o", "Ko", "Mo", "Go", "To"];
    let mut v = n as f64;
    let mut i = 0;
    while v >= 1000.0 && i < U.len() - 1 {
        v /= 1000.0;
        i += 1;
    }
    if i == 0 {
        format!("{n} o")
    } else {
        format!("{v:.1} {}", U[i])
    }
}
