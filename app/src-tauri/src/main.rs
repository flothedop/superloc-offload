// Fenêtre sans console sous Windows en version publiée.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Couche Tauri : relie l'interface au moteur. Aucune logique de copie ici ;
//! tout est dans `offload-core`.

use offload_core::gabarit::{self, Valeurs};
use offload_core::hash::Algo;
use offload_core::inventaire;
use offload_core::langue::Langue;
use offload_core::moteur::{self, Annulation, Evenement, Options};
use offload_core::rapport::{ModeVerif, Rapport};
use offload_core::registre::{self, Registre};
use offload_core::verification;
use offload_core::volume::{self, Volume};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_dialog::DialogExt;

/* -------------------------------------------------------------- Réglages */

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
struct Reglages {
    /// « fr », « en », ou vide pour suivre le système.
    langue: String,
    operateur: String,
    email: String,
    projet: String,
    jour: String,
    camera: String,
    lieu: String,
    gabarit: String,
    /// Dossiers de base habituels.
    destinations: Vec<String>,
    /// Ceux qui sont cochés.
    actives: Vec<String>,
    mode: ModeVerif,
    algos: Vec<Algo>,
    mhl_v1: bool,
    son: bool,
    notification: bool,
    ejecter: bool,
    avertissement_accepte: bool,
}

impl Default for Reglages {
    fn default() -> Self {
        Reglages {
            langue: String::new(),
            operateur: String::new(),
            email: String::new(),
            projet: String::new(),
            jour: String::new(),
            camera: String::new(),
            lieu: String::new(),
            gabarit: gabarit::PAR_DEFAUT.into(),
            destinations: Vec::new(),
            actives: Vec::new(),
            mode: ModeVerif::Complet,
            algos: offload_core::hash::PAR_DEFAUT.to_vec(),
            mhl_v1: true,
            son: true,
            notification: true,
            ejecter: false,
            avertissement_accepte: false,
        }
    }
}

fn dossier_donnees(app: &AppHandle) -> PathBuf {
    app.path()
        .app_data_dir()
        .unwrap_or_else(|_| std::env::temp_dir().join("superloc-offload"))
}

fn fichier_reglages(app: &AppHandle) -> PathBuf {
    app.path()
        .app_config_dir()
        .unwrap_or_else(|_| dossier_donnees(app))
        .join("reglages.json")
}

#[tauri::command]
fn reglages_lire(app: AppHandle) -> Reglages {
    std::fs::read_to_string(fichier_reglages(&app))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

#[tauri::command]
fn reglages_ecrire(app: AppHandle, reglages: Reglages) -> Result<(), String> {
    let f = fichier_reglages(&app);
    if let Some(p) = f.parent() {
        std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
    }
    let temp = f.with_extension("json.tmp");
    std::fs::write(
        &temp,
        serde_json::to_vec_pretty(&reglages).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    std::fs::rename(&temp, &f).map_err(|e| e.to_string())
}

/* --------------------------------------------------------------- Volumes */

/// Reconnaît la structure d'une carte caméra courante.
fn structure_camera(racine: &Path) -> Option<&'static str> {
    let entrees: Vec<String> = std::fs::read_dir(racine)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    let a = |nom: &str| entrees.iter().any(|e| e.eq_ignore_ascii_case(nom));
    let fin = |suffixe: &str| {
        entrees
            .iter()
            .any(|e| e.to_ascii_uppercase().ends_with(suffixe))
    };
    if fin(".RDM") || fin(".RDC") {
        return Some("RED");
    }
    if racine.join("PRIVATE").join("M4ROOT").is_dir() {
        return Some("Sony XAVC");
    }
    if a("XDROOT") || a("MEDIAPRO.XML") {
        return Some("Sony XDCAM");
    }
    if racine.join("CONTENTS").join("CLIPS001").is_dir() {
        return Some("Canon");
    }
    if fin("NC-ZR") || a("NIKON") {
        return Some("Nikon");
    }
    if fin(".BRAW") || a("BRAW") {
        return Some("Blackmagic");
    }
    if a("DCIM") {
        return Some("DCIM");
    }
    // Bobine ARRI / Codex : A001R1AB, contenant des MXF.
    if entrees.iter().any(|e| {
        let b = e.as_bytes();
        e.len() >= 8
            && b[0].is_ascii_uppercase()
            && b[1..4].iter().all(u8::is_ascii_digit)
            && (b[4] == b'R' || b[4] == b'C')
    }) {
        return Some("ARRI");
    }
    None
}

#[derive(Serialize)]
struct VolumeUi {
    #[serde(flatten)]
    volume: Volume,
    camera: Option<&'static str>,
}

#[tauri::command]
async fn volumes() -> Vec<VolumeUi> {
    tauri::async_runtime::spawn_blocking(|| {
        volume::lister()
            .into_iter()
            .filter(|v| !v.reseau || v.amovible)
            .map(|v| VolumeUi {
                camera: structure_camera(Path::new(&v.point_montage)),
                volume: v,
            })
            .collect()
    })
    .await
    .unwrap_or_default()
}

#[derive(Serialize)]
struct Source {
    chemin: String,
    nom: String,
    nb_fichiers: usize,
    octets: u64,
    nb_clips: usize,
    dossiers_vides: usize,
    camera: Option<&'static str>,
    volume: Option<Volume>,
    exclus: usize,
    illisibles: Vec<String>,
    segments_manquants: Vec<String>,
    deja: Vec<registre::Entree>,
}

#[tauri::command]
async fn analyser_source(app: AppHandle, chemin: String) -> Result<Source, String> {
    let registre = Registre::new(&dossier_donnees(&app));
    tauri::async_runtime::spawn_blocking(move || {
        let p = PathBuf::from(&chemin);
        let inv = inventaire::inventorier(&p).map_err(|e| e.message(Langue::Fr))?;
        let opts = Options::new(&p, Vec::new());
        let empreinte = registre::empreinte_inventaire(&inv);
        Ok(Source {
            nom: opts.nom,
            nb_fichiers: inv.fichiers.len(),
            octets: inv.octets_annonces,
            nb_clips: inv.nb_clips(),
            dossiers_vides: inv.dossiers_vides().len(),
            camera: structure_camera(&p),
            volume: volume::du_chemin(&p),
            exclus: inv.exclus.len(),
            illisibles: inv
                .illisibles
                .iter()
                .map(|e| e.message(Langue::Fr))
                .collect(),
            segments_manquants: inv.segments_manquants(),
            deja: registre.chercher(&empreinte),
            chemin,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[derive(Serialize)]
struct Apercu {
    base: String,
    dossier: String,
    volume: Option<Volume>,
    existe_base: bool,
}

#[derive(Deserialize)]
struct DemandeApercu {
    bases: Vec<String>,
    gabarit: String,
    valeurs: Valeurs,
}

fn dossiers_finaux(bases: &[PathBuf], gabarit_: &str, valeurs: &Valeurs) -> Vec<PathBuf> {
    let rel = gabarit::nom_libre(bases, &gabarit::rendre(gabarit_, valeurs));
    gabarit::dossiers(bases, &rel)
}

#[tauri::command]
async fn apercu(demande: DemandeApercu) -> Vec<Apercu> {
    tauri::async_runtime::spawn_blocking(move || {
        let bases: Vec<PathBuf> = demande.bases.iter().map(PathBuf::from).collect();
        let finaux = dossiers_finaux(&bases, &demande.gabarit, &demande.valeurs);
        bases
            .iter()
            .zip(finaux)
            .map(|(b, f)| Apercu {
                base: b.display().to_string(),
                dossier: f.display().to_string(),
                volume: b.is_dir().then(|| volume::du_chemin(b)).flatten(),
                existe_base: b.is_dir(),
            })
            .collect()
    })
    .await
    .unwrap_or_default()
}

#[tauri::command]
async fn choisir_dossier(app: AppHandle) -> Option<String> {
    let (tx, mut rx) = tauri::async_runtime::channel(1);
    app.dialog().file().pick_folder(move |choix| {
        let _ = tx.blocking_send(choix);
    });
    rx.recv()
        .await
        .flatten()
        .and_then(|f| f.into_path().ok())
        .map(|p| p.display().to_string())
}

/* ------------------------------------------------------------ Déchargement */

#[derive(Default)]
struct Tache {
    annulation: Mutex<Option<Annulation>>,
}

#[derive(Deserialize)]
struct Demande {
    source: String,
    nom: String,
    bases: Vec<String>,
    gabarit: String,
    valeurs: Valeurs,
    mode: ModeVerif,
    algos: Vec<Algo>,
    mhl_v1: bool,
    email: String,
    lieu: String,
    commentaire: String,
    langue: String,
    ejecter: bool,
}

#[derive(Serialize)]
struct ProblemeUi {
    chemin: String,
    destination: Option<usize>,
    message: String,
    action: String,
}

#[derive(Serialize)]
#[serde(tag = "issue", rename_all = "kebab-case")]
enum Issue {
    Rapport {
        rapport: Box<Rapport>,
        noms: Vec<String>,
        remarques: Vec<String>,
        problemes: Vec<ProblemeUi>,
        ejection: Option<Result<(), String>>,
    },
    Refus {
        erreurs: Vec<ErreurUi>,
    },
}

#[derive(Serialize)]
struct ErreurUi {
    code: offload_core::erreur::Code,
    message: String,
    action: String,
}

fn eveil() -> Option<keepawake::KeepAwake> {
    keepawake::Builder::default()
        .idle(true)
        .sleep(true)
        .reason("Copie de rushs en cours")
        .app_name("SuperLoc Offload")
        .app_reverse_domain("fr.superloc.offload")
        .create()
        .ok()
}

#[tauri::command]
async fn decharger(
    app: AppHandle,
    tache: State<'_, Tache>,
    demande: Demande,
) -> Result<Issue, String> {
    let jeton = Annulation::default();
    {
        let mut a = tache.annulation.lock().map_err(|e| e.to_string())?;
        if a.is_some() {
            return Err("une copie est déjà en cours".into());
        }
        *a = Some(jeton.clone());
    }
    let relais = app.clone();
    let registre = Registre::new(&dossier_donnees(&app));
    let resultat = tauri::async_runtime::spawn_blocking(move || {
        let _eveil = eveil();
        let l = Langue::depuis(&demande.langue);
        let source = PathBuf::from(&demande.source);
        let bases: Vec<PathBuf> = demande.bases.iter().map(PathBuf::from).collect();
        let mut valeurs = demande.valeurs.clone();
        valeurs.carte = demande.nom.clone();
        if valeurs.date.is_empty() {
            valeurs.date = chrono::Local::now().format("%Y-%m-%d").to_string();
        }
        let mut o = Options::new(&source, dossiers_finaux(&bases, &demande.gabarit, &valeurs));
        o.nom = demande.nom.clone();
        o.algos = demande.algos.clone();
        o.mode = demande.mode;
        o.mhl_v1 = demande.mhl_v1;
        o.operateur = valeurs.operateur.clone();
        o.email = demande.email.clone();
        o.projet = valeurs.projet.clone();
        o.lieu = demande.lieu.clone();
        o.commentaire = demande.commentaire.clone();
        o.langue = l;
        o.annulation = jeton;
        let r = moteur::decharger(&o, &mut |e: Evenement| {
            let _ = relais.emit("offload://evenement", enrichir(&e, l));
        });
        match r {
            Ok(rapport) => {
                let _ = registre.ajouter(&rapport);
                let ejection =
                    (demande.ejecter && !rapport.annule).then(|| ejecter_volume(&source));
                let noms = rapport.noms_destinations();
                let remarques = rapport
                    .remarques
                    .iter()
                    .map(|r| r.texte(l, &noms))
                    .collect();
                let mut problemes = Vec::new();
                for e in &rapport.illisibles {
                    problemes.push(ProblemeUi {
                        chemin: e.detail.clone(),
                        destination: None,
                        message: e.message(l),
                        action: e.action(l).into(),
                    });
                }
                for f in &rapport.fichiers {
                    if let Some(e) = &f.erreur_source {
                        problemes.push(ProblemeUi {
                            chemin: f.chemin.clone(),
                            destination: None,
                            message: e.message(l),
                            action: e.action(l).into(),
                        });
                        continue;
                    }
                    for c in &f.copies {
                        if let Some(e) = &c.erreur {
                            problemes.push(ProblemeUi {
                                chemin: f.chemin.clone(),
                                destination: Some(c.destination),
                                message: e.message(l),
                                action: e.action(l).into(),
                            });
                        }
                    }
                }
                Issue::Rapport {
                    rapport: Box::new(rapport),
                    noms,
                    remarques,
                    problemes,
                    ejection,
                }
            }
            Err(refus) => Issue::Refus {
                erreurs: refus
                    .erreurs
                    .iter()
                    .map(|e| ErreurUi {
                        code: e.code,
                        message: e.message(l),
                        action: e.action(l).to_string(),
                    })
                    .collect(),
            },
        }
    })
    .await
    .map_err(|e| e.to_string());
    if let Ok(mut a) = tache.annulation.lock() {
        *a = None;
    }
    resultat
}

/// Ajoute à l'événement le message et l'action traduits de son erreur.
fn enrichir(e: &Evenement, l: Langue) -> serde_json::Value {
    let mut v = serde_json::to_value(e).unwrap_or_default();
    let erreur = match e {
        Evenement::Copie {
            erreur: Some(x), ..
        } => Some(x),
        Evenement::Source { erreur, .. } => Some(erreur),
        Evenement::Destination {
            erreur: Some(x), ..
        } => Some(x),
        _ => None,
    };
    if let (Some(x), Some(o)) = (erreur, v.as_object_mut()) {
        o.insert("message".into(), x.message(l).into());
        o.insert("action".into(), x.action(l).into());
    }
    v
}

#[tauri::command]
fn annuler(tache: State<'_, Tache>) -> bool {
    match tache.annulation.lock().ok().and_then(|a| a.clone()) {
        Some(j) => {
            j.annuler();
            true
        }
        None => false,
    }
}

/* ----------------------------------------------------------- Vérification */

#[derive(Serialize)]
struct BilanUi {
    #[serde(flatten)]
    bilan: verification::Bilan,
    conforme: bool,
    problemes_texte: Vec<(String, String)>,
}

#[tauri::command]
async fn verifier_dossier(
    app: AppHandle,
    tache: State<'_, Tache>,
    chemin: String,
    langue: String,
) -> Result<BilanUi, String> {
    let jeton = Annulation::default();
    {
        let mut a = tache.annulation.lock().map_err(|e| e.to_string())?;
        if a.is_some() {
            return Err("une opération est déjà en cours".into());
        }
        *a = Some(jeton.clone());
    }
    let relais = app.clone();
    let r = tauri::async_runtime::spawn_blocking(move || {
        let _eveil = eveil();
        let l = Langue::depuis(&langue);
        verification::verifier(Path::new(&chemin), &jeton, &mut |p| {
            let _ = relais.emit("offload://verification", &p);
        })
        .map(|b| BilanUi {
            conforme: b.conforme(),
            problemes_texte: b
                .problemes
                .iter()
                .map(|p| (p.chemin.clone(), p.erreur.message(l)))
                .collect(),
            bilan: b,
        })
        .map_err(|e| format!("{}\n{}", e.message(l), e.action(l)))
    })
    .await
    .map_err(|e| e.to_string())?;
    if let Ok(mut a) = tache.annulation.lock() {
        *a = None;
    }
    r
}

#[tauri::command]
fn registre_lire(app: AppHandle) -> Vec<registre::Entree> {
    let mut v = Registre::new(&dossier_donnees(&app)).entrees();
    v.reverse();
    v
}

/* ---------------------------------------------------------------- Système */

#[tauri::command]
fn ouvrir(app: AppHandle, chemin: String) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_path(chemin, None::<&str>)
        .map_err(|e| e.to_string())
}

/// Éjecte le volume qui porte `chemin`.
fn ejecter_volume(chemin: &Path) -> Result<(), String> {
    let point = volume::du_chemin(chemin)
        .map(|v| v.point_montage)
        .ok_or("volume introuvable")?;
    let sortie = if cfg!(target_os = "macos") {
        std::process::Command::new("diskutil")
            .args(["eject", &point])
            .output()
    } else if cfg!(windows) {
        let lettre = point.trim_end_matches('\\').to_string();
        let script = format!(
            "(New-Object -comObject Shell.Application).Namespace(17).ParseName('{lettre}').InvokeVerb('Eject')"
        );
        std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .output()
    } else {
        std::process::Command::new("gio")
            .args(["mount", "-e", &point])
            .output()
    }
    .map_err(|e| e.to_string())?;
    if !sortie.status.success() {
        return Err(String::from_utf8_lossy(&sortie.stderr).trim().to_string());
    }
    // Windows ne dit rien : on vérifie que le volume a bien disparu.
    std::thread::sleep(std::time::Duration::from_millis(1500));
    if Path::new(&point).exists() && cfg!(windows) {
        return Err("le système n'a pas éjecté le volume".into());
    }
    Ok(())
}

#[tauri::command]
async fn ejecter(chemin: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || ejecter_volume(Path::new(&chemin)))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
fn infos() -> serde_json::Value {
    serde_json::json!({
        "version": offload_core::VERSION,
        "os": std::env::consts::OS,
    })
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .manage(Tache::default())
        .setup(|app| {
            // Essai de bout en bout, réservé aux tests automatisés :
            // OFFLOAD_AUTO="carte|disque1|disque2".
            if let (Ok(auto), Some(f)) = (
                std::env::var("OFFLOAD_AUTO"),
                app.get_webview_window("main"),
            ) {
                let bouts: Vec<String> = auto.split('|').map(str::to_string).collect();
                if bouts.len() >= 2 {
                    let carte = serde_json::to_string(&bouts[0]).unwrap_or_default();
                    let disques = serde_json::to_string(&bouts[1..]).unwrap_or_default();
                    std::thread::spawn(move || {
                        std::thread::sleep(std::time::Duration::from_millis(1500));
                        let _ = f.eval(format!(
                            "window.__essai && window.__essai({carte}, {disques})"
                        ));
                    });
                }
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            reglages_lire,
            reglages_ecrire,
            volumes,
            analyser_source,
            apercu,
            choisir_dossier,
            decharger,
            annuler,
            verifier_dossier,
            registre_lire,
            ouvrir,
            ejecter,
            infos
        ])
        .run(tauri::generate_context!())
        .expect("le lancement de l'application a échoué");
}
