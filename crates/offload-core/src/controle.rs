//! Contrôles préalables : tout ce qui peut être refusé avant d'écrire le
//! moindre octet doit l'être. Tous les problèmes sont listés d'un coup, pas
//! seulement le premier.

use crate::erreur::{Code, Erreur};
use crate::inventaire::Inventaire;
use crate::noms;
use crate::volume::{self, Volume};
use std::path::{Component, Path, PathBuf};

/// 4 Gio moins un octet : taille maximale d'un fichier en FAT32.
pub const LIMITE_FAT32: u64 = 4 * 1024 * 1024 * 1024 - 1;
/// Place réservée aux manifestes et rapports, en plus des rushs.
const MARGE_FIXE: u64 = 64 * 1024 * 1024;

#[derive(Debug)]
pub struct Controle {
    pub volumes: Vec<Option<Volume>>,
}

/// Forme absolue d'un chemin qui peut ne pas encore exister : on résout le
/// plus proche parent existant, puis on rajoute le reste.
pub fn absolu(p: &Path) -> PathBuf {
    let mut existant = p.to_path_buf();
    let mut reste: Vec<std::ffi::OsString> = Vec::new();
    loop {
        if let Ok(c) = dunce::canonicalize(&existant) {
            let mut r = c;
            for x in reste.iter().rev() {
                r.push(x);
            }
            return r;
        }
        match (
            existant.file_name().map(|n| n.to_os_string()),
            existant.parent(),
        ) {
            (Some(n), Some(parent)) => {
                reste.push(n);
                existant = parent.to_path_buf();
            }
            _ => return p.to_path_buf(),
        }
    }
}

fn parent_existant(p: &Path) -> Option<PathBuf> {
    p.ancestors().find(|a| a.is_dir()).map(Path::to_path_buf)
}

fn est_dans(enfant: &Path, parent: &Path) -> bool {
    let e: Vec<Component> = enfant.components().collect();
    let p: Vec<Component> = parent.components().collect();
    e.len() >= p.len()
        && e.iter().zip(p.iter()).all(|(a, b)| {
            if cfg!(any(windows, target_os = "macos")) {
                a.as_os_str().to_string_lossy().to_lowercase()
                    == b.as_os_str().to_string_lossy().to_lowercase()
            } else {
                a == b
            }
        })
}

/// Essaie de créer puis d'effacer un fichier dans le dossier : détecte un
/// disque en lecture seule (NTFS sur Mac sans pilote, carte verrouillée).
fn inscriptible(dossier: &Path) -> Result<(), std::io::Error> {
    let sonde = dossier.join(format!(".offload-sonde-{}", std::process::id()));
    let f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&sonde)?;
    drop(f);
    let _ = std::fs::remove_file(&sonde);
    Ok(())
}

pub fn controler(
    inv: &Inventaire,
    source: &Path,
    destinations: &[PathBuf],
) -> Result<Controle, Vec<Erreur>> {
    let mut erreurs = Vec::new();
    if destinations.is_empty() {
        return Err(vec![Erreur::new(Code::AucuneDestination, "")]);
    }
    if inv.fichiers.is_empty() && inv.dossiers.is_empty() {
        erreurs.push(Erreur::new(Code::SourceVide, source.display().to_string()));
    }

    let source_abs = absolu(source);
    let absolus: Vec<PathBuf> = destinations.iter().map(|d| absolu(d)).collect();
    let mut volumes = Vec::new();

    for (i, (d, abs)) in destinations.iter().zip(&absolus).enumerate() {
        if d.exists() {
            erreurs.push(Erreur::new(
                Code::DestinationExiste,
                d.display().to_string(),
            ));
        }
        if est_dans(abs, &source_abs) {
            erreurs.push(Erreur::new(
                Code::DestinationDansSource,
                d.display().to_string(),
            ));
        }
        if est_dans(&source_abs, abs) {
            erreurs.push(Erreur::new(
                Code::SourceDansDestination,
                d.display().to_string(),
            ));
        }
        if absolus[..i]
            .iter()
            .any(|a| est_dans(a, abs) || est_dans(abs, a))
        {
            erreurs.push(Erreur::new(
                Code::DestinationsIdentiques,
                d.display().to_string(),
            ));
        }
        let base = parent_existant(d);
        let vol = base.as_deref().and_then(volume::du_chemin);
        if let Some(b) = &base {
            if !d.exists() {
                if let Err(e) = inscriptible(b) {
                    erreurs.push(Erreur::io(
                        Code::CreationImpossible,
                        format!("{} (disque en lecture seule ?)", b.display()),
                        &e,
                    ));
                }
            }
        } else {
            erreurs.push(Erreur::new(
                Code::CreationImpossible,
                d.display().to_string(),
            ));
        }
        volumes.push(vol);
    }

    // Espace : les destinations d'un même volume s'additionnent.
    let besoin = inv.octets_annonces + inv.octets_annonces / 100 + MARGE_FIXE;
    let mut deja: Vec<(String, u64)> = Vec::new();
    for (d, v) in destinations.iter().zip(&volumes) {
        let Some(base) = parent_existant(d) else {
            continue;
        };
        let libre = fs4::available_space(&base)
            .ok()
            .or(v.as_ref().map(|v| v.libre));
        let cle = v
            .as_ref()
            .map(|v| v.point_montage.clone())
            .unwrap_or_else(|| base.display().to_string());
        let cumul = match deja.iter_mut().find(|(c, _)| *c == cle) {
            Some((_, n)) => {
                *n += besoin;
                *n
            }
            None => {
                deja.push((cle, besoin));
                besoin
            }
        };
        if let Some(libre) = libre {
            if libre < cumul {
                erreurs.push(Erreur::new(
                    Code::EspaceInsuffisant,
                    format!(
                        "{} : {} libres, {} nécessaires",
                        d.display(),
                        crate::langue::octets(libre, crate::langue::Langue::Fr),
                        crate::langue::octets(cumul, crate::langue::Langue::Fr)
                    ),
                ));
            }
        }
    }

    // Limites du système de fichiers de chaque destination.
    let chemins: Vec<&str> = inv
        .dossiers
        .iter()
        .map(|d| d.chemin.as_str())
        .chain(inv.fichiers.iter().map(|f| f.chemin.as_str()))
        .collect();
    for (d, v) in destinations.iter().zip(&volumes) {
        let systeme = v
            .as_ref()
            .map(|v| v.systeme_fichiers.as_str())
            .unwrap_or("");
        if v.as_ref().map(|v| v.limite_4_gio()).unwrap_or(false) {
            for f in inv
                .fichiers
                .iter()
                .filter(|f| f.taille_annoncee > LIMITE_FAT32)
            {
                erreurs.push(Erreur::new(
                    Code::FichierTropGrosFat32,
                    format!("{} → {}", f.chemin, d.display()),
                ));
            }
        }
        if noms::regles_windows(systeme) {
            for c in &chemins {
                for composant in c.split('/') {
                    if let Some(raison) = noms::refus_windows(composant) {
                        erreurs.push(Erreur::new(
                            Code::NomInvalide,
                            format!("{c} ({raison}) → {}", d.display()),
                        ));
                        break;
                    }
                }
            }
        }
        let casse = !systeme.is_empty() && noms::sensible_a_la_casse(systeme);
        for (a, b) in noms::collisions(chemins.iter().copied(), casse) {
            erreurs.push(Erreur::new(
                Code::CollisionNoms,
                format!("« {a} » et « {b} » → {}", d.display()),
            ));
        }
    }

    // Une même erreur de nom peut concerner plusieurs destinations : on
    // garde tout, la liste est lue de haut en bas.
    if erreurs.is_empty() {
        Ok(Controle { volumes })
    } else {
        Err(erreurs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inventaire::{inventorier, FichierCarte};

    fn inv_simple() -> (tempfile::TempDir, Inventaire) {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir(d.path().join("carte")).unwrap();
        std::fs::write(d.path().join("carte/a.mov"), b"abc").unwrap();
        let inv = inventorier(&d.path().join("carte")).unwrap();
        (d, inv)
    }

    #[test]
    fn destination_existante_refusee() {
        let (d, inv) = inv_simple();
        std::fs::create_dir(d.path().join("dest")).unwrap();
        let e = controler(&inv, &d.path().join("carte"), &[d.path().join("dest")]).unwrap_err();
        assert!(e.iter().any(|e| e.code == Code::DestinationExiste));
    }

    #[test]
    fn destination_dans_la_carte_refusee() {
        let (d, inv) = inv_simple();
        let e = controler(
            &inv,
            &d.path().join("carte"),
            &[d.path().join("carte/copie")],
        )
        .unwrap_err();
        assert!(e.iter().any(|e| e.code == Code::DestinationDansSource));
    }

    #[test]
    fn deux_fois_la_meme_destination_refusee() {
        let (d, inv) = inv_simple();
        let x = d.path().join("x");
        let e = controler(&inv, &d.path().join("carte"), &[x.clone(), x]).unwrap_err();
        assert!(e.iter().any(|e| e.code == Code::DestinationsIdentiques));
    }

    #[test]
    fn espace_insuffisant_refuse() {
        let (d, mut inv) = inv_simple();
        inv.octets_annonces = u64::MAX / 4;
        let e = controler(&inv, &d.path().join("carte"), &[d.path().join("x")]).unwrap_err();
        assert!(e.iter().any(|e| e.code == Code::EspaceInsuffisant));
    }

    #[test]
    fn destination_valide_acceptee() {
        let (d, inv) = inv_simple();
        let c = controler(
            &inv,
            &d.path().join("carte"),
            &[d.path().join("a"), d.path().join("b")],
        )
        .unwrap();
        assert_eq!(c.volumes.len(), 2);
        assert!(c.volumes[0].is_some());
    }

    #[test]
    fn collisions_unicode_detectees() {
        let (d, mut inv) = inv_simple();
        for nom in ["Sc\u{e9}ne.mov", "Sce\u{301}ne.mov"] {
            inv.fichiers.push(FichierCarte {
                chemin: nom.into(),
                taille_annoncee: 1,
                modifie: None,
                cree: None,
                clip: nom.into(),
            });
        }
        let e = controler(&inv, &d.path().join("carte"), &[d.path().join("x")]).unwrap_err();
        assert!(e.iter().any(|e| e.code == Code::CollisionNoms));
    }

    #[test]
    fn chemin_absolu_dun_dossier_a_creer() {
        let d = tempfile::tempdir().unwrap();
        let a = absolu(&d.path().join("x/y"));
        assert!(a.ends_with("x/y"));
        assert!(a.is_absolute());
    }
}
