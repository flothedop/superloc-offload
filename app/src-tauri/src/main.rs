// Fenêtre sans console sous Windows en version publiée.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Couche Tauri : relie l'interface au moteur de déchargement.
//!
//! Le moteur reste entièrement dans `offload-core` — ce fichier ne fait que
//! traduire ses structures pour l'interface et relayer l'avancement.

use offload_core::copy::{self, Options, Progression};
use offload_core::hash::Algo;
use offload_core::report::{Etat, Rapport};
use offload_core::{scan, verify};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_dialog::DialogExt;

/* ------------------------------------------------------------------ Types */

#[derive(Serialize, Clone)]
struct FichierUi {
    chemin: String,
    clip: String,
    taille: u64,
}

#[derive(Serialize, Clone)]
struct Carte {
    nom: String,
    chemin: String,
    structure: String,
    clips: usize,
    fichiers: Vec<FichierUi>,
    octets: u64,
}

#[derive(Serialize, Deserialize, Clone)]
struct Disque {
    nom: String,
    chemin: String,
    total: u64,
    libre: u64,
}

#[derive(Deserialize)]
struct SourceUi {
    nom: String,
    chemin: String,
}

#[derive(Deserialize)]
struct OptionsUi {
    source: SourceUi,
    destinations: Vec<Disque>,
    empreinte: String,
    #[serde(rename = "verifierSource")]
    verifier_source: bool,
}

#[derive(Serialize, Clone)]
struct DestinationUi {
    nom: String,
    dossier: String,
    conforme: bool,
    fichiers_ok: usize,
    fichiers_echec: usize,
    debit_moyen: f64,
    systeme_fichiers: Option<String>,
}

#[derive(Serialize, Clone)]
struct RapportUi {
    conforme: bool,
    nb_fichiers: usize,
    nb_clips: usize,
    octets_total: u64,
    destinations: Vec<DestinationUi>,
    avertissements: Vec<String>,
    dossier_rapport: Option<String>,
    debit_source: f64,
    diagnostic: offload_core::diagnostic::Diagnostic,
}

/* ------------------------------------------------------------- Détection */

/// Reconnaît la caméra d'origine à la forme de l'arborescence.
fn structure_carte(inventaire: &scan::Scan) -> String {
    let chemins: Vec<&str> = inventaire
        .fichiers
        .iter()
        .map(|f| f.chemin.as_str())
        .collect();
    let contient = |motif: &str| {
        chemins
            .iter()
            .any(|c| c.to_ascii_uppercase().contains(motif))
    };

    if contient(".RDC/") || contient(".R3D") {
        "RED".to_string()
    } else if contient("XDROOT") || contient("PRIVATE/M4ROOT") {
        "Sony".to_string()
    } else if contient(".ARI") || contient("ARRI/") {
        "ARRI".to_string()
    } else if contient(".BRAW") {
        "Blackmagic".to_string()
    } else if contient("DCIM/") {
        "Carte photo".to_string()
    } else {
        "Dossier".to_string()
    }
}

fn algos_depuis(texte: &str) -> Vec<Algo> {
    let v: Vec<Algo> = texte.split(',').filter_map(Algo::parse).collect();
    if v.is_empty() {
        vec![Algo::Xxh64]
    } else {
        v
    }
}

async fn demander_dossier(app: &AppHandle) -> Option<PathBuf> {
    let (tx, mut rx) = tauri::async_runtime::channel(1);
    app.dialog().file().pick_folder(move |choix| {
        let _ = tx.blocking_send(choix);
    });
    rx.recv().await.flatten().and_then(|f| f.into_path().ok())
}

/* ----------------------------------------------------------- Commandes */

/// Prend le chemin fourni (dossier déposé sur la fenêtre) ou ouvre le
/// sélecteur de dossier.
async fn dossier_ou_dialogue(app: &AppHandle, chemin: Option<String>) -> Option<PathBuf> {
    match chemin {
        Some(c) => Some(PathBuf::from(c)),
        None => demander_dossier(app).await,
    }
}

#[tauri::command]
async fn choisir_carte(app: AppHandle, chemin: Option<String>) -> Result<Option<Carte>, String> {
    let Some(chemin) = dossier_ou_dialogue(&app, chemin).await else {
        return Ok(None);
    };

    // Le parcours d'une grande carte prend du temps : il ne doit pas figer
    // l'interface.
    let resultat = tauri::async_runtime::spawn_blocking(move || {
        let inventaire = scan::scanner(&chemin)?;
        let structure = structure_carte(&inventaire);
        Ok::<_, std::io::Error>(Carte {
            nom: chemin
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "carte".to_string()),
            chemin: chemin.display().to_string(),
            structure,
            clips: inventaire.nb_clips(),
            octets: inventaire.octets_total,
            fichiers: inventaire
                .fichiers
                .iter()
                .map(|f| FichierUi {
                    chemin: f.chemin.clone(),
                    clip: f.clip.clone(),
                    taille: f.taille,
                })
                .collect(),
        })
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;

    Ok(Some(resultat))
}

#[tauri::command]
async fn choisir_disque(app: AppHandle, chemin: Option<String>) -> Result<Option<Disque>, String> {
    let Some(chemin) = dossier_ou_dialogue(&app, chemin).await else {
        return Ok(None);
    };
    let total = fs4::total_space(&chemin).map_err(|e| e.to_string())?;
    let libre = fs4::available_space(&chemin).map_err(|e| e.to_string())?;
    Ok(Some(Disque {
        nom: chemin
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| chemin.display().to_string()),
        chemin: chemin.display().to_string(),
        total,
        libre,
    }))
}

#[tauri::command]
async fn decharger(app: AppHandle, options: OptionsUi) -> Result<RapportUi, String> {
    let source = PathBuf::from(&options.source.chemin);
    let nom = options.source.nom.clone();
    let noms: Vec<String> = options.destinations.iter().map(|d| d.nom.clone()).collect();
    let cibles: Vec<PathBuf> = options
        .destinations
        .iter()
        .map(|d| Path::new(&d.chemin).join(&nom))
        .collect();

    let opts = Options {
        source,
        destinations: cibles.clone(),
        algos: algos_depuis(&options.empreinte),
        verifier_source: options.verifier_source,
        nom: nom.clone(),
    };

    let relais = app.clone();
    let rapport: Rapport = tauri::async_runtime::spawn_blocking(move || {
        copy::decharger(&opts, &mut |p| match p {
            Progression::Demarrage {
                nb_fichiers,
                octets_total,
            } => {
                let _ = relais.emit(
                    "offload://demarrage",
                    serde_json::json!({ "nb_fichiers": nb_fichiers, "octets_total": octets_total }),
                );
            }
            Progression::Fichier {
                index,
                total,
                chemin,
            } => {
                let _ = relais.emit(
                    "offload://fichier",
                    serde_json::json!({ "index": index, "total": total, "chemin": chemin }),
                );
            }
            Progression::Octets {
                copies,
                total,
                debit,
            } => {
                let _ = relais.emit(
                    "offload://octets",
                    serde_json::json!({ "copies": copies, "total": total, "debit": debit }),
                );
            }
            Progression::Debit {
                destination,
                debit,
                cumul,
                fichiers_faits,
            } => {
                let _ = relais.emit(
                    "offload://debit",
                    serde_json::json!({
                        "destination": destination,
                        "debit": debit,
                        "cumul": cumul,
                        "fichiers_faits": fichiers_faits
                    }),
                );
            }
            Progression::Diagnostic(diag) => {
                let _ = relais.emit("offload://diagnostic", &diag);
            }
            Progression::Verifie {
                destination,
                index,
                chemin,
                etat,
                erreur,
            } => {
                let motif = erreur.map(|e| {
                    format!(
                        "{} : {e}",
                        noms.get(destination).cloned().unwrap_or_default()
                    )
                });
                let _ = relais.emit(
                    "offload://resultat",
                    serde_json::json!({
                        "destination": destination,
                        "index": index,
                        "chemin": chemin,
                        "etat": if etat.ok() { "ok" } else { "echec" },
                        "motif": motif
                    }),
                );
            }
        })
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;

    // Dépôt du MHL et du rapport sur chaque destination.
    let mut rapport = rapport;
    let horodatage = chrono_local();
    for (i, cible) in cibles.iter().enumerate() {
        if !cible.is_dir() {
            continue;
        }
        let nom_mhl = offload_core::mhl::nom_fichier(&nom, &horodatage);
        match offload_core::mhl::ecrire(&rapport, i, &cible.join(&nom_mhl)) {
            Ok(_) => rapport.destinations[i].mhl = Some(nom_mhl),
            Err(e) => rapport
                .avertissements
                .push(format!("MHL non écrit sur {} : {e}", cible.display())),
        }
        if let Err(e) = rapport.ecrire_json(&cible.join(format!("{nom}_rapport.json"))) {
            rapport
                .avertissements
                .push(format!("rapport non écrit sur {} : {e}", cible.display()));
        }
    }

    Ok(RapportUi {
        conforme: rapport.etat_global.ok(),
        nb_fichiers: rapport.nb_fichiers,
        nb_clips: rapport.nb_clips,
        octets_total: rapport.octets_total,
        destinations: rapport
            .destinations
            .iter()
            .enumerate()
            .map(|(i, d)| DestinationUi {
                nom: options
                    .destinations
                    .get(i)
                    .map(|x| x.nom.clone())
                    .unwrap_or_else(|| d.dossier.clone()),
                dossier: d.dossier.clone(),
                conforme: d.etat == Etat::Ok,
                fichiers_ok: d.fichiers_ok,
                fichiers_echec: d.fichiers_echec,
                debit_moyen: d.profil.debit_moyen,
                systeme_fichiers: d.volume.as_ref().map(|v| v.systeme_fichiers.clone()),
            })
            .collect(),
        avertissements: rapport.avertissements.clone(),
        dossier_rapport: cibles.first().map(|c| c.display().to_string()),
        debit_source: rapport.profil_source.debit_moyen,
        diagnostic: offload_core::diagnostic::depuis_rapport(&rapport),
    })
}

fn chrono_local() -> chrono::DateTime<chrono::Local> {
    chrono::Local::now()
}

#[tauri::command]
async fn verifier_dossier(
    app: AppHandle,
    chemin: Option<String>,
) -> Result<Option<verify::RapportVerif>, String> {
    let Some(chemin) = dossier_ou_dialogue(&app, chemin).await else {
        return Ok(None);
    };
    let r = tauri::async_runtime::spawn_blocking(move || verify::controler_dossier(&chemin))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    Ok(Some(r))
}

#[tauri::command]
fn ouvrir_dossier(app: AppHandle, chemin: String) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_path(chemin, None::<&str>)
        .map_err(|e| e.to_string())
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let Some(fenetre) = app.get_webview_window("main") else {
                return Ok(());
            };
            let _ = fenetre.set_title("SuperLoc Offload");

            // Essai automatique de bout en bout, réservé aux tests.
            if let Ok(auto) = std::env::var("OFFLOAD_AUTO") {
                let bouts: Vec<String> = auto.split('|').map(|s| s.to_string()).collect();
                if bouts.len() >= 2 {
                    let carte = serde_json::to_string(&bouts[0]).unwrap_or_default();
                    let disques = serde_json::to_string(&bouts[1..]).unwrap_or_default();
                    let f = fenetre.clone();
                    std::thread::spawn(move || {
                        std::thread::sleep(std::time::Duration::from_millis(1200));
                        let _ = f.eval(format!("window.__essai({carte}, {disques})"));
                    });
                }
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            choisir_carte,
            choisir_disque,
            decharger,
            verifier_dossier,
            ouvrir_dossier
        ])
        .run(tauri::generate_context!())
        .expect("le lancement de l'application a échoué");
}
