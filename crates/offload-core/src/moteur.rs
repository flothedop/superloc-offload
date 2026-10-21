//! Moteur de déchargement.
//!
//! ```text
//!  carte ─lecture─► réserve de tampons ─┬─► fil d'empreinte (un par algorithme)
//!                                       ├─► destination A : écrit → synchronise → chasse du cache
//!                                       │                   → mesure → relit → compare → renomme
//!                                       └─► destination B : idem, à son rythme
//! ```
//!
//! La lecture de la carte est faite par le fil appelant, qui orchestre aussi
//! les événements : le rappel n'est jamais appelé depuis un autre fil.

use crate::ascmhl;
use crate::controle;
use crate::diagnostic;
use crate::erreur::{Code, Erreur};
use crate::es::{self, Durabilite, Residence};
use crate::faute::Plan;
use crate::hash::{self, Algo, Empreinte, Hacheur};
use crate::inventaire::{self, Inventaire};
use crate::langue::Langue;
use crate::mesure::{Cumul, Debitmetre, ProfilDebit};
use crate::mhl1;
use crate::rapport::{
    Copie, DestinationRapport, FichierRapport, ModeVerif, Niveau, Rapport, Verdict,
};
use crate::rapport_html;
use crate::sanscache;
use crate::tampon::{Bloc, Reserve};
use crate::volume::{self, Volume};
use crate::{BLOC, DOSSIER_RAPPORT, OUTIL, SUFFIXE_TEMP, TAMPONS, VERSION};
use chrono::{DateTime, Utc};
use crossbeam_channel::{unbounded, Receiver, Sender};
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime};

const PERIODE_AVANCEMENT: Duration = Duration::from_millis(100);
const PERIODE_DIAGNOSTIC: Duration = Duration::from_secs(15);
const PERIODE_PROFIL: Duration = Duration::from_secs(3);

// ------------------------------------------------------------------------
// Options et événements
// ------------------------------------------------------------------------

/// Jeton d'annulation, partagé avec l'interface.
#[derive(Clone, Default, Debug)]
pub struct Annulation(Arc<AtomicBool>);

impl Annulation {
    pub fn annuler(&self) {
        self.0.store(true, Ordering::SeqCst);
    }
    pub fn demandee(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

#[derive(Clone, Debug)]
pub struct Options {
    pub source: PathBuf,
    /// Dossier final du déchargement sur chaque destination ; il ne doit pas
    /// exister.
    pub destinations: Vec<PathBuf>,
    /// Nom de la carte, repris dans les rapports.
    pub nom: String,
    pub algos: Vec<Algo>,
    pub mode: ModeVerif,
    /// Écrire aussi un MHL v1.1.
    pub mhl_v1: bool,
    pub operateur: String,
    pub email: String,
    pub projet: String,
    pub lieu: String,
    pub commentaire: String,
    pub langue: Langue,
    pub annulation: Annulation,
    pub fautes: Plan,
}

impl Options {
    pub fn new(source: impl Into<PathBuf>, destinations: Vec<PathBuf>) -> Options {
        let source = source.into();
        Options {
            nom: volume::du_chemin(&source)
                .filter(|v| Path::new(&v.point_montage) == source.as_path())
                .map(|v| v.nom)
                .or_else(|| source.file_name().map(|n| n.to_string_lossy().to_string()))
                .unwrap_or_else(|| "CARTE".into()),
            source,
            destinations,
            algos: hash::PAR_DEFAUT.to_vec(),
            mode: ModeVerif::Complet,
            mhl_v1: true,
            operateur: String::new(),
            email: String::new(),
            projet: String::new(),
            lieu: String::new(),
            commentaire: String::new(),
            langue: Langue::Fr,
            annulation: Annulation::default(),
            fautes: Plan::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Phase {
    Inventaire,
    Controle,
    Copie,
    RelectureCarte,
    Rapports,
    Termine,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct AvancementDestination {
    pub ecrits: u64,
    pub relus: u64,
    pub debit: f64,
    pub fichiers_ok: usize,
    pub fichiers_echec: usize,
    pub en_panne: bool,
    pub termine: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Avancement {
    pub lus: u64,
    pub total: u64,
    pub debit_lecture: f64,
    pub index: usize,
    pub nb_fichiers: usize,
    pub chemin: String,
    pub destinations: Vec<AvancementDestination>,
    pub ecoule_s: f64,
    pub restant_s: Option<f64>,
    /// Relecture finale de la carte : octets relus.
    pub relecture_carte: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Evenement {
    Phase {
        phase: Phase,
    },
    Debut {
        nom: String,
        nb_fichiers: usize,
        octets: u64,
        nb_clips: usize,
        destinations: Vec<String>,
        volumes: Vec<Option<Volume>>,
    },
    Avancement(Avancement),
    Copie {
        destination: usize,
        index: usize,
        chemin: String,
        niveau: Niveau,
        erreur: Option<Erreur>,
    },
    Source {
        index: usize,
        chemin: String,
        erreur: Erreur,
    },
    Diagnostic(diagnostic::Diagnostic),
    Destination {
        destination: usize,
        termine: bool,
        erreur: Option<Erreur>,
    },
}

/// Échec avant toute copie : inventaire ou contrôles refusés.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Refus {
    pub erreurs: Vec<Erreur>,
}

// ------------------------------------------------------------------------
// Promesse d'empreinte : la carte a été lue, les empreintes arrivent
// ------------------------------------------------------------------------

#[derive(Default)]
struct EtatPromesse {
    taille: Option<u64>,
    erreur: Option<Erreur>,
    empreintes: Vec<Empreinte>,
}

struct Promesse {
    etat: Mutex<EtatPromesse>,
    cv: Condvar,
    attendues: usize,
}

impl Promesse {
    fn new(attendues: usize) -> Arc<Promesse> {
        Arc::new(Promesse {
            etat: Mutex::new(EtatPromesse::default()),
            cv: Condvar::new(),
            attendues,
        })
    }

    fn verrou(&self) -> std::sync::MutexGuard<'_, EtatPromesse> {
        self.etat.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn fournir_lecture(&self, r: Result<u64, Erreur>) {
        let mut e = self.verrou();
        match r {
            Ok(t) => e.taille = Some(t),
            Err(x) => e.erreur = Some(x),
        }
        self.cv.notify_all();
    }

    fn fournir_empreinte(&self, emp: Empreinte) {
        let mut e = self.verrou();
        e.empreintes.push(emp);
        self.cv.notify_all();
    }

    /// Attend la fin de la lecture et toutes les empreintes.
    fn attendre(&self, annulation: &Annulation) -> Result<(u64, Vec<Empreinte>), Erreur> {
        let mut e = self.verrou();
        loop {
            if let Some(x) = &e.erreur {
                return Err(x.clone());
            }
            if let Some(t) = e.taille {
                if e.empreintes.len() >= self.attendues {
                    let mut v = e.empreintes.clone();
                    v.sort_by_key(|x| x.algo);
                    return Ok((t, v));
                }
            }
            if annulation.demandee() {
                return Err(Erreur::new(Code::Annule, ""));
            }
            e = self
                .cv
                .wait_timeout(e, Duration::from_millis(100))
                .map(|(g, _)| g)
                .unwrap_or_else(|p| p.into_inner().0);
        }
    }
}

// ------------------------------------------------------------------------
// Fils d'empreinte
// ------------------------------------------------------------------------

enum VersHacheur {
    Debut,
    Bloc(Arc<Bloc>),
    Fin(Arc<Promesse>),
}

fn fil_empreinte(algo: Algo, rx: Receiver<VersHacheur>) {
    let mut courant: Option<Hacheur> = None;
    while let Ok(m) = rx.recv() {
        match m {
            VersHacheur::Debut => courant = Some(Hacheur::new(algo)),
            VersHacheur::Bloc(b) => {
                if let Some(h) = courant.as_mut() {
                    h.update(&b);
                }
            }
            VersHacheur::Fin(p) => {
                if let Some(h) = courant.take() {
                    p.fournir_empreinte(Empreinte {
                        algo,
                        valeur: h.finir(),
                    });
                }
            }
        }
    }
}

// ------------------------------------------------------------------------
// Fils de destination
// ------------------------------------------------------------------------

enum VersDestination {
    Debut {
        index: usize,
        relatif: Arc<str>,
    },
    Bloc(Arc<Bloc>),
    Fin {
        promesse: Arc<Promesse>,
        modifie: Option<DateTime<Utc>>,
        cree: Option<DateTime<Utc>>,
    },
    Fini,
}

enum Retour {
    Copie {
        destination: usize,
        index: usize,
        chemin: Arc<str>,
        copie: Copie,
    },
    Termine {
        destination: usize,
        erreur: Option<Erreur>,
    },
}

#[derive(Default)]
struct Compteurs {
    ecrits: AtomicU64,
    relus: AtomicU64,
    debit_bits: AtomicU64,
    fichiers_ok: AtomicUsize,
    fichiers_echec: AtomicUsize,
    en_panne: AtomicBool,
    termine: AtomicBool,
    profil: Mutex<ProfilDebit>,
}

impl Compteurs {
    fn lire(&self) -> AvancementDestination {
        AvancementDestination {
            ecrits: self.ecrits.load(Ordering::Relaxed),
            relus: self.relus.load(Ordering::Relaxed),
            debit: f64::from_bits(self.debit_bits.load(Ordering::Relaxed)),
            fichiers_ok: self.fichiers_ok.load(Ordering::Relaxed),
            fichiers_echec: self.fichiers_echec.load(Ordering::Relaxed),
            en_panne: self.en_panne.load(Ordering::Relaxed),
            termine: self.termine.load(Ordering::Relaxed),
        }
    }
}

#[derive(Default)]
struct Bilan {
    erreur_fatale: Option<Erreur>,
    octets_ecrits: u64,
    profil: ProfilDebit,
    temps_ecriture_ms: u64,
    temps_sync_ms: u64,
    temps_verif_ms: u64,
    manifestes: Vec<String>,
    durabilite_degradee: bool,
}

struct Contexte {
    destination: usize,
    racine: PathBuf,
    dossiers: Arc<Vec<inventaire::DossierCarte>>,
    algos: Vec<Algo>,
    mode: ModeVerif,
    mhl_v1: bool,
    debut: DateTime<Utc>,
    createur: ascmhl::Createur,
    infos_mhl1: (String, String, String),
    annulation: Annulation,
    fautes: Plan,
    compteurs: Arc<Compteurs>,
    retour: Sender<Retour>,
}

struct EnCours {
    index: usize,
    relatif: Arc<str>,
    temp: PathBuf,
    final_: PathBuf,
    fichier: Option<File>,
    ecrits: u64,
    erreur: Option<Erreur>,
}

fn chemin_temporaire(final_: &Path) -> PathBuf {
    let mut nom = final_.file_name().unwrap_or_default().to_os_string();
    nom.push(SUFFIXE_TEMP);
    final_.with_file_name(nom)
}

fn joindre(racine: &Path, relatif: &str) -> PathBuf {
    relatif
        .split('/')
        .fold(racine.to_path_buf(), |p, s| p.join(s))
}

fn effacer(p: &Path) {
    let _ = es::avec_patience(|| fs::remove_file(p));
}

/// Erreur d'écriture qui rend la destination inutilisable pour la suite.
fn fatale(e: &io::Error) -> bool {
    if crate::erreur::est_disque_plein(e) {
        return true;
    }
    matches!(
        e.kind(),
        io::ErrorKind::NotFound
            | io::ErrorKind::ReadOnlyFilesystem
            | io::ErrorKind::PermissionDenied
            | io::ErrorKind::NotConnected
    ) || {
        #[cfg(unix)]
        {
            matches!(
                e.raw_os_error(),
                Some(libc::EIO) | Some(libc::ENODEV) | Some(libc::ENXIO)
            )
        }
        #[cfg(not(unix))]
        {
            // ERROR_NOT_READY (21), ERROR_DEV_NOT_EXIST (55), ERROR_DEVICE_NOT_CONNECTED (1167).
            matches!(e.raw_os_error(), Some(21) | Some(55) | Some(1167))
        }
    }
}

/// Relit la copie et compare. Renvoie le niveau obtenu et les infos de
/// lecture, ou l'erreur.
fn verifier_copie(
    ctx: &Contexte,
    temp: &Path,
    taille: u64,
    empreintes: &[Empreinte],
    verif: &mut Cumul,
) -> Result<(Niveau, Option<sanscache::Mode>, Option<Residence>), Erreur> {
    let longueur = fs::metadata(temp)
        .map_err(|e| Erreur::io(Code::RelectureImpossible, temp.display(), &e))?
        .len();
    if longueur != taille {
        return Err(Erreur::new(
            Code::TailleDifferente,
            format!(
                "{} : {longueur} octets sur la copie, {taille} lus sur la carte",
                temp.display()
            ),
        ));
    }
    if ctx.mode == ModeVerif::Taille {
        return Ok((Niveau::Taille, None, None));
    }

    // Chasse du cache puis mesure : c'est cette mesure qui fonde le niveau
    // « relue depuis le disque » sur macOS.
    let residence = es::evincer(temp).ok();

    let algo = hash::de_comparaison(&ctx.algos);
    let attendu = empreintes
        .iter()
        .find(|e| e.algo == algo)
        .map(|e| e.valeur.clone())
        .ok_or_else(|| Erreur::new(Code::Interne, "empreinte source absente"))?;

    let debut = Instant::now();
    let mut lecteur = sanscache::ouvrir(temp, BLOC)
        .map_err(|e| Erreur::io(Code::RelectureImpossible, temp.display(), &e))?;
    let mode = lecteur.mode();
    let mut h = Hacheur::new(algo);
    let mut relus = 0u64;
    loop {
        if ctx.annulation.demandee() {
            return Err(Erreur::new(Code::Annule, temp.display().to_string()));
        }
        let bloc = lecteur
            .lire_bloc()
            .map_err(|e| Erreur::io(Code::RelectureImpossible, temp.display(), &e))?;
        if bloc.is_empty() {
            break;
        }
        h.update(bloc);
        relus += bloc.len() as u64;
        ctx.compteurs
            .relus
            .fetch_add(bloc.len() as u64, Ordering::Relaxed);
    }
    verif.ajouter(debut.elapsed());
    let obtenu = h.finir();
    if relus != taille {
        return Err(Erreur::new(
            Code::TailleDifferente,
            format!(
                "{} : {relus} octets relus, {taille} attendus",
                temp.display()
            ),
        ));
    }
    if obtenu != attendu {
        return Err(Erreur::new(
            Code::EmpreinteDifferente,
            format!(
                "{} : {} {obtenu} au lieu de {attendu}",
                temp.display(),
                algo.nom()
            ),
        ));
    }
    let prouve = mode.garantit_le_support() || residence.map(|r| r.hors_cache()).unwrap_or(false);
    let niveau = if prouve {
        Niveau::Support
    } else {
        Niveau::CachePossible
    };
    Ok((niveau, Some(mode), residence))
}

fn fil_destination(ctx: Contexte, rx: Receiver<VersDestination>) -> Bilan {
    let mut bilan = Bilan::default();
    let mut debitmetre = Debitmetre::new();
    let (mut ecriture, mut sync, mut verif) =
        (Cumul::default(), Cumul::default(), Cumul::default());
    let mut courant: Option<EnCours> = None;
    let mut entrees: Vec<ascmhl::Entree> = Vec::new();
    let mut derniere_publication = Instant::now();
    let mut dernier_profil = Instant::now();
    let d = ctx.destination;

    let marquer_panne = |bilan: &mut Bilan, e: Erreur| {
        if bilan.erreur_fatale.is_none() {
            ctx.compteurs.en_panne.store(true, Ordering::Relaxed);
            bilan.erreur_fatale = Some(e);
        }
    };

    // Création du dossier du déchargement : `create_dir` et non
    // `create_dir_all` pour la racine, afin d'échouer s'il est apparu entre
    // le contrôle et maintenant.
    let creation = (|| -> io::Result<()> {
        if let Some(p) = ctx.racine.parent() {
            fs::create_dir_all(p)?;
        }
        fs::create_dir(&ctx.racine)?;
        for dossier in ctx.dossiers.iter() {
            fs::create_dir_all(joindre(&ctx.racine, &dossier.chemin))?;
        }
        Ok(())
    })();
    if let Err(e) = creation {
        let code = if e.kind() == io::ErrorKind::AlreadyExists {
            Code::DestinationExiste
        } else {
            Code::CreationImpossible
        };
        marquer_panne(&mut bilan, Erreur::io(code, ctx.racine.display(), &e));
    }

    let envoyer_copie = |index: usize, chemin: Arc<str>, copie: Copie| {
        if copie.niveau == Niveau::Echec {
            ctx.compteurs.fichiers_echec.fetch_add(1, Ordering::Relaxed);
        } else {
            ctx.compteurs.fichiers_ok.fetch_add(1, Ordering::Relaxed);
        }
        let _ = ctx.retour.send(Retour::Copie {
            destination: d,
            index,
            chemin,
            copie,
        });
    };
    let echec = |erreur: Erreur| Copie {
        destination: d,
        niveau: Niveau::Echec,
        erreur: Some(erreur),
        durabilite: None,
        lecture: None,
        residence: None,
    };

    while let Ok(message) = rx.recv() {
        match message {
            VersDestination::Debut { index, relatif } => {
                let final_ = joindre(&ctx.racine, &relatif);
                let temp = chemin_temporaire(&final_);
                let mut e = EnCours {
                    index,
                    relatif: relatif.clone(),
                    temp: temp.clone(),
                    final_,
                    fichier: None,
                    ecrits: 0,
                    erreur: bilan.erreur_fatale.clone(),
                };
                if e.erreur.is_none() && ctx.fautes.debranche(d, &relatif) {
                    let x = Erreur::new(Code::DestinationPerdue, ctx.racine.display().to_string());
                    marquer_panne(&mut bilan, x.clone());
                    e.erreur = Some(x);
                }
                if e.erreur.is_none() {
                    match es::creer_exclusif(&temp) {
                        Ok(f) => e.fichier = Some(f),
                        Err(x) => {
                            let err = Erreur::io(Code::CreationImpossible, temp.display(), &x);
                            if fatale(&x) {
                                marquer_panne(&mut bilan, err.clone());
                            }
                            e.erreur = Some(err);
                        }
                    }
                }
                courant = Some(e);
            }

            VersDestination::Bloc(bloc) => {
                let Some(e) = courant.as_mut() else { continue };
                if e.erreur.is_some() {
                    continue;
                }
                let Some(f) = e.fichier.as_mut() else {
                    continue;
                };
                if let Some(ms) = ctx.fautes.lenteur(d) {
                    std::thread::sleep(Duration::from_millis(ms));
                }
                let r = if ctx.fautes.disque_plein(d, bilan.octets_ecrits) {
                    Err(io::Error::from(io::ErrorKind::StorageFull))
                } else if let Some(i) = ctx.fautes.bit_ecrit(d, &e.relatif, e.ecrits, bloc.len()) {
                    let mut copie = bloc.to_vec();
                    copie[i] ^= 0x01;
                    ecriture.mesurer(|| f.write_all(&copie))
                } else {
                    ecriture.mesurer(|| f.write_all(&bloc))
                };
                match r {
                    Ok(()) => {
                        let n = bloc.len() as u64;
                        e.ecrits += n;
                        bilan.octets_ecrits += n;
                        debitmetre.ajouter(n);
                        ctx.compteurs.ecrits.fetch_add(n, Ordering::Relaxed);
                    }
                    Err(x) => {
                        let err = Erreur::io(Code::EcritureImpossible, e.temp.display(), &x);
                        if fatale(&x) {
                            marquer_panne(&mut bilan, err.clone());
                        }
                        e.erreur = Some(err);
                    }
                }
            }

            VersDestination::Fin {
                promesse,
                modifie,
                cree,
            } => {
                let Some(mut e) = courant.take() else {
                    continue;
                };
                let mut durabilite = None;

                if e.erreur.is_none() && ctx.fautes.ecriture_courte(d, &e.relatif) {
                    if let Some(f) = e.fichier.as_ref() {
                        let _ = f.set_len(e.ecrits.saturating_sub(1000));
                    }
                }
                if e.erreur.is_none() {
                    if let Some(f) = e.fichier.as_ref() {
                        match sync.mesurer(|| es::synchroniser(f)) {
                            Ok(dur) => {
                                if dur == Durabilite::Degradee {
                                    bilan.durabilite_degradee = true;
                                }
                                durabilite = Some(dur);
                            }
                            Err(x) => {
                                let err = Erreur::io(Code::SynchroImpossible, e.temp.display(), &x);
                                if fatale(&x) {
                                    marquer_panne(&mut bilan, err.clone());
                                }
                                e.erreur = Some(err);
                            }
                        }
                    }
                }
                // La date de la carte, après la dernière écriture.
                if e.erreur.is_none() {
                    if let (Some(f), Some(m)) = (e.fichier.as_ref(), modifie) {
                        let _ = es::appliquer_date(f, SystemTime::from(m));
                    }
                }
                e.fichier = None;

                // La lecture de la carte et ses empreintes.
                let source = promesse.attendre(&ctx.annulation);
                let resultat: Result<Copie, Erreur> = match (&e.erreur, source) {
                    (Some(x), _) => Err(x.clone()),
                    (None, Err(x)) => Err(x),
                    (None, Ok((taille, empreintes))) => {
                        match verifier_copie(&ctx, &e.temp, taille, &empreintes, &mut verif) {
                            Err(x) => Err(x),
                            Ok((niveau, lecture, residence)) => {
                                match es::avec_patience(|| {
                                    if e.final_.exists() {
                                        return Err(io::Error::new(
                                            io::ErrorKind::AlreadyExists,
                                            "le fichier définitif existe déjà",
                                        ));
                                    }
                                    fs::rename(&e.temp, &e.final_)
                                }) {
                                    Err(x) => Err(Erreur::io(
                                        Code::RenommageImpossible,
                                        e.final_.display(),
                                        &x,
                                    )),
                                    Ok(()) => {
                                        entrees.push(ascmhl::Entree {
                                            chemin: e.relatif.to_string(),
                                            taille,
                                            modifie,
                                            cree,
                                            empreintes,
                                            date_empreinte: Utc::now(),
                                            action: "original",
                                        });
                                        Ok(Copie {
                                            destination: d,
                                            niveau,
                                            erreur: None,
                                            durabilite,
                                            lecture,
                                            residence,
                                        })
                                    }
                                }
                            }
                        }
                    }
                };
                let copie = match resultat {
                    Ok(c) => c,
                    Err(x) => {
                        effacer(&e.temp);
                        echec(x)
                    }
                };
                envoyer_copie(e.index, e.relatif.clone(), copie);
            }

            VersDestination::Fini => break,
        }

        if derniere_publication.elapsed() >= Duration::from_millis(250) {
            derniere_publication = Instant::now();
            ctx.compteurs
                .debit_bits
                .store(debitmetre.debit().to_bits(), Ordering::Relaxed);
            if dernier_profil.elapsed() >= PERIODE_PROFIL {
                dernier_profil = Instant::now();
                if let Ok(mut p) = ctx.compteurs.profil.lock() {
                    *p = debitmetre.bilan();
                }
            }
        }
    }

    // Fichier interrompu (annulation) : jamais laissé sous un nom ambigu.
    if let Some(e) = courant.take() {
        drop(e.fichier);
        effacer(&e.temp);
        envoyer_copie(
            e.index,
            e.relatif.clone(),
            echec(Erreur::new(Code::Annule, "")),
        );
    }

    if ctx.racine.is_dir() {
        // Dates des dossiers, les plus profonds d'abord (créer un fichier
        // modifie la date de son dossier).
        let mut dossiers: Vec<&inventaire::DossierCarte> = ctx.dossiers.iter().collect();
        dossiers.sort_by_key(|x| std::cmp::Reverse(x.chemin.matches('/').count()));
        for x in &dossiers {
            let p = joindre(&ctx.racine, &x.chemin);
            es::synchroniser_dossier(&p);
            if let Some(m) = x.modifie {
                let _ = es::appliquer_date_dossier(&p, SystemTime::from(m));
            }
        }
        es::synchroniser_dossier(&ctx.racine);

        // Manifestes : seuls les fichiers vérifiés présents sur ce disque.
        let dossiers_mhl: Vec<ascmhl::Dossier> = ctx
            .dossiers
            .iter()
            .map(|x| ascmhl::Dossier {
                chemin: x.chemin.clone(),
                modifie: x.modifie,
            })
            .collect();
        let feuille = ctx
            .racine
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let mut motifs = inventaire::motifs_exclusion();
        motifs.push(format!("{DOSSIER_RAPPORT}/"));
        if ctx.mhl_v1 && mhl1::possible(&ctx.algos) {
            motifs.push(mhl1::nom_fichier(&feuille, &ctx.debut));
        }
        match ascmhl::ecrire(
            &ctx.racine,
            &ctx.debut,
            &ctx.createur,
            &entrees,
            &dossiers_mhl,
            &motifs,
            &ctx.algos,
        ) {
            Ok(rel) => bilan.manifestes.push(rel),
            Err(x) => marquer_panne(&mut bilan, x),
        }
        if ctx.mhl_v1 && mhl1::possible(&ctx.algos) {
            let (nom, utilisateur, machine) = &ctx.infos_mhl1;
            let outil = format!("{OUTIL} {VERSION}");
            let infos = mhl1::Infos {
                nom,
                utilisateur,
                machine,
                outil: &outil,
                debut: ctx.debut,
                fin: Utc::now(),
            };
            match mhl1::ecrire(&ctx.racine, &infos, &entrees) {
                Ok(rel) => bilan.manifestes.push(rel),
                Err(x) => marquer_panne(&mut bilan, x),
            }
        }
    }

    bilan.profil = debitmetre.bilan();
    bilan.temps_ecriture_ms = ecriture.ms();
    bilan.temps_sync_ms = sync.ms();
    bilan.temps_verif_ms = verif.ms();
    ctx.compteurs
        .debit_bits
        .store(0f64.to_bits(), Ordering::Relaxed);
    ctx.compteurs.termine.store(true, Ordering::Relaxed);
    let _ = ctx.retour.send(Retour::Termine {
        destination: d,
        erreur: bilan.erreur_fatale.clone(),
    });
    bilan
}

// ------------------------------------------------------------------------
// Orchestration
// ------------------------------------------------------------------------

struct Destination {
    tx: Sender<VersDestination>,
    fil: Option<JoinHandle<Bilan>>,
    compteurs: Arc<Compteurs>,
    attente: Cumul,
}

struct Etat<'a> {
    rappel: &'a mut dyn FnMut(Evenement),
    fichiers: Vec<FichierRapport>,
    journal: Vec<String>,
    noms: Vec<String>,
}

impl Etat<'_> {
    fn noter(&mut self, ligne: String) {
        let h = chrono::Local::now().format("%H:%M:%S");
        self.journal.push(format!("{h}  {ligne}"));
    }

    fn appliquer(&mut self, r: Retour) {
        match r {
            Retour::Copie {
                destination,
                index,
                chemin,
                copie,
            } => {
                let nom = self.noms.get(destination).cloned().unwrap_or_default();
                let ligne = match &copie.erreur {
                    Some(e) => format!("ÉCHEC  {chemin} → {nom} : {}", e.message(Langue::Fr)),
                    None => format!("{}  {chemin} → {nom}", copie.niveau.libelle(Langue::Fr)),
                };
                self.noter(ligne);
                (self.rappel)(Evenement::Copie {
                    destination,
                    index,
                    chemin: chemin.to_string(),
                    niveau: copie.niveau,
                    erreur: copie.erreur.clone(),
                });
                if let Some(f) = self.fichiers.get_mut(index) {
                    f.copies.retain(|c| c.destination != destination);
                    f.copies.push(copie);
                }
            }
            Retour::Termine {
                destination,
                erreur,
            } => {
                let nom = self.noms.get(destination).cloned().unwrap_or_default();
                self.noter(match &erreur {
                    Some(e) => format!("{nom} : terminé en échec : {}", e.message(Langue::Fr)),
                    None => format!("{nom} : terminé, manifestes écrits"),
                });
                (self.rappel)(Evenement::Destination {
                    destination,
                    termine: true,
                    erreur,
                });
            }
        }
    }
}

fn vider(rx: &Receiver<Retour>, etat: &mut Etat) {
    while let Ok(r) = rx.try_recv() {
        etat.appliquer(r);
    }
}

/// Lit un fichier de la carte jusqu'au bout et le distribue.
#[allow(clippy::too_many_arguments)]
fn lire_fichier(
    opts: &Options,
    relatif: &str,
    reserve: &Reserve,
    hacheurs: &[Sender<VersHacheur>],
    destinations: &mut [Destination],
    lus_total: &mut u64,
    debit: &mut Debitmetre,
    lecture: &mut Cumul,
    attente_hachage: &mut Cumul,
    entre_blocs: &mut dyn FnMut(&mut [Destination], u64, f64),
) -> Result<u64, Erreur> {
    let chemin = joindre(&opts.source, relatif);
    let avant = fs::metadata(&chemin).map_err(|e| Erreur::io(Code::LectureSource, relatif, &e))?;
    let mut f = File::open(&chemin).map_err(|e| Erreur::io(Code::LectureSource, relatif, &e))?;
    let mut lus = 0u64;

    loop {
        // Contre-pression : on attend un tampon libre. Le temps attendu est
        // imputé à celui qui a le plus de retard.
        let mut tampon = loop {
            if opts.annulation.demandee() {
                return Err(Erreur::new(Code::Annule, relatif));
            }
            let t0 = Instant::now();
            match reserve.prendre(Duration::from_millis(20)) {
                Some(t) => {
                    let attendu = t0.elapsed();
                    if attendu > Duration::from_micros(500) {
                        imputer(destinations, hacheurs, attendu, attente_hachage);
                    }
                    break t;
                }
                None => {
                    imputer(destinations, hacheurs, t0.elapsed(), attente_hachage);
                    entre_blocs(destinations, *lus_total, debit.debit());
                }
            }
        };

        // Remplit le tampon jusqu'au bout ou jusqu'à la fin du fichier.
        let capacite = tampon.capacite().min(BLOC);
        let mut n = 0usize;
        let t0 = Instant::now();
        while n < capacite {
            match f.read(&mut tampon.zone()[n..capacite]) {
                Ok(0) => break,
                Ok(k) => n += k,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => {
                    drop(reserve.emballer(tampon));
                    return Err(Erreur::io(Code::LectureSource, relatif, &e));
                }
            }
        }
        lecture.ajouter(t0.elapsed());
        if opts.fautes.lecture_source(relatif, lus + n as u64) {
            drop(reserve.emballer(tampon));
            return Err(Erreur::new(
                Code::LectureSource,
                format!("{relatif} : erreur d'entrée-sortie (simulée)"),
            ));
        }
        if n == 0 {
            drop(reserve.emballer(tampon));
            break;
        }
        tampon.fixer_longueur(n);
        let bloc = Arc::new(reserve.emballer(tampon));
        for h in hacheurs {
            let _ = h.send(VersHacheur::Bloc(Arc::clone(&bloc)));
        }
        for d in destinations.iter() {
            let _ = d.tx.send(VersDestination::Bloc(Arc::clone(&bloc)));
        }
        drop(bloc);
        lus += n as u64;
        *lus_total += n as u64;
        debit.ajouter(n as u64);
        if let Some(obs) = &opts.fautes.apres_bloc_source {
            obs(relatif, lus);
        }
        entre_blocs(destinations, *lus_total, debit.debit());
        if n < capacite {
            break;
        }
    }

    // La carte a-t-elle bougé pendant la lecture ?
    let apres = fs::metadata(&chemin).map_err(|e| Erreur::io(Code::LectureSource, relatif, &e))?;
    let change = avant.len() != apres.len() || avant.modified().ok() != apres.modified().ok();
    if change {
        return Err(Erreur::new(
            Code::SourceModifiee,
            format!(
                "{relatif} : {} → {} octets annoncés",
                avant.len(),
                apres.len()
            ),
        ));
    }
    Ok(lus)
}

fn imputer(
    destinations: &mut [Destination],
    hacheurs: &[Sender<VersHacheur>],
    duree: Duration,
    attente_hachage: &mut Cumul,
) {
    let retard_hachage = hacheurs.iter().map(|h| h.len()).max().unwrap_or(0);
    let plus_en_retard = destinations
        .iter_mut()
        .filter(|d| !d.compteurs.en_panne.load(Ordering::Relaxed))
        .max_by_key(|d| d.tx.len());
    match plus_en_retard {
        Some(d) if d.tx.len() >= retard_hachage => d.attente.ajouter(duree),
        _ => attente_hachage.ajouter(duree),
    }
}

/// Exécute un déchargement complet.
///
/// `Err` seulement si rien n'a été écrit (inventaire ou contrôles refusés).
/// Dès que la copie commence, un rapport est toujours rendu, verdict compris.
pub fn decharger(opts: &Options, rappel: &mut dyn FnMut(Evenement)) -> Result<Rapport, Refus> {
    let debut = Utc::now();
    let horloge = Instant::now();
    let algos = hash::normaliser(&opts.algos);

    rappel(Evenement::Phase {
        phase: Phase::Inventaire,
    });
    let inv: Inventaire =
        inventaire::inventorier(&opts.source).map_err(|e| Refus { erreurs: vec![e] })?;

    rappel(Evenement::Phase {
        phase: Phase::Controle,
    });
    let controle = controle::controler(&inv, &opts.source, &opts.destinations)
        .map_err(|erreurs| Refus { erreurs })?;

    let volume_source = volume::du_chemin(&opts.source);
    let noms = crate::rapport::noms_affiches(&opts.destinations, &controle.volumes);

    // Garde le poste éveillé pendant la copie : délégué à l'appelant (CLI,
    // application), qui possède le cycle de vie.
    let machine = whoami::fallible::hostname().unwrap_or_else(|_| "inconnu".into());
    let utilisateur = whoami::username();
    let createur = ascmhl::Createur {
        date: debut,
        machine: machine.clone(),
        outil: OUTIL.into(),
        version: VERSION.into(),
        auteur: (!opts.operateur.trim().is_empty()).then(|| ascmhl::Auteur {
            nom: opts.operateur.clone(),
            email: (!opts.email.trim().is_empty()).then(|| opts.email.clone()),
            telephone: None,
            role: None,
        }),
        lieu: (!opts.lieu.trim().is_empty()).then(|| opts.lieu.clone()),
        commentaire: {
            let mut c = Vec::new();
            if !opts.projet.trim().is_empty() {
                c.push(format!("Projet : {}", opts.projet));
            }
            c.push(format!("Carte : {}", opts.nom));
            if !opts.commentaire.trim().is_empty() {
                c.push(opts.commentaire.clone());
            }
            Some(c.join(" — "))
        },
    };

    let reserve = match Reserve::new(TAMPONS, BLOC) {
        Ok(r) => r,
        Err(e) => {
            return Err(Refus {
                erreurs: vec![Erreur::io(Code::Interne, "mémoire", &e)],
            })
        }
    };

    // Fils d'empreinte.
    let mut hacheurs: Vec<Sender<VersHacheur>> = Vec::new();
    let mut fils_hachage = Vec::new();
    for a in &algos {
        let (tx, rx) = unbounded();
        let algo = *a;
        hacheurs.push(tx);
        fils_hachage.push(std::thread::spawn(move || fil_empreinte(algo, rx)));
    }

    // Fils de destination.
    let (tx_retour, rx_retour) = unbounded::<Retour>();
    let dossiers = Arc::new(inv.dossiers.clone());
    let mut destinations: Vec<Destination> = opts
        .destinations
        .iter()
        .enumerate()
        .map(|(i, racine)| {
            let (tx, rx) = unbounded();
            let compteurs = Arc::new(Compteurs::default());
            let ctx = Contexte {
                destination: i,
                racine: racine.clone(),
                dossiers: Arc::clone(&dossiers),
                algos: algos.clone(),
                mode: opts.mode,
                mhl_v1: opts.mhl_v1,
                debut,
                createur: createur.clone(),
                infos_mhl1: (opts.nom.clone(), utilisateur.clone(), machine.clone()),
                annulation: opts.annulation.clone(),
                fautes: opts.fautes.clone(),
                compteurs: Arc::clone(&compteurs),
                retour: tx_retour.clone(),
            };
            Destination {
                tx,
                fil: Some(std::thread::spawn(move || fil_destination(ctx, rx))),
                compteurs,
                attente: Cumul::default(),
            }
        })
        .collect();
    drop(tx_retour);

    let mut etat = Etat {
        rappel,
        fichiers: inv
            .fichiers
            .iter()
            .map(|f| FichierRapport {
                chemin: f.chemin.clone(),
                clip: f.clip.clone(),
                taille: f.taille_annoncee,
                taille_annoncee: f.taille_annoncee,
                modifie: f.modifie,
                cree: f.cree,
                empreintes: Vec::new(),
                erreur_source: None,
                copies: Vec::new(),
                relecture_carte: None,
            })
            .collect(),
        journal: Vec::new(),
        noms: noms.clone(),
    };
    etat.noter(format!(
        "{OUTIL} {VERSION} — carte « {} » ({}) : {} fichiers, {}",
        opts.nom,
        opts.source.display(),
        inv.fichiers.len(),
        crate::langue::octets(inv.octets_annonces, Langue::Fr)
    ));
    for (i, d) in opts.destinations.iter().enumerate() {
        etat.noter(format!(
            "Destination {} : {} ({})",
            i + 1,
            d.display(),
            noms[i]
        ));
    }

    (etat.rappel)(Evenement::Debut {
        nom: opts.nom.clone(),
        nb_fichiers: inv.fichiers.len(),
        octets: inv.octets_annonces,
        nb_clips: inv.nb_clips(),
        destinations: opts
            .destinations
            .iter()
            .map(|d| d.display().to_string())
            .collect(),
        volumes: controle.volumes.clone(),
    });
    (etat.rappel)(Evenement::Phase {
        phase: Phase::Copie,
    });

    let mut lus_total = 0u64;
    let mut debit_source = Debitmetre::new();
    let mut lecture = Cumul::default();
    let mut attente_hachage = Cumul::default();
    let mut dernier_avancement = Instant::now();
    let mut dernier_diagnostic = Instant::now();
    let total = inv.octets_annonces;
    let nb = inv.fichiers.len();
    let taille_mediane = {
        let mut t: Vec<u64> = inv.fichiers.iter().map(|f| f.taille_annoncee).collect();
        t.sort_unstable();
        t.get(t.len() / 2).copied().unwrap_or(0)
    };
    let mut annule = false;
    let mut promesses: Vec<(usize, Arc<Promesse>)> = Vec::new();

    for (index, f) in inv.fichiers.iter().enumerate() {
        if opts.annulation.demandee() {
            annule = true;
            break;
        }
        let relatif: Arc<str> = Arc::from(f.chemin.as_str());
        for h in &hacheurs {
            let _ = h.send(VersHacheur::Debut);
        }
        for d in &destinations {
            let _ = d.tx.send(VersDestination::Debut {
                index,
                relatif: Arc::clone(&relatif),
            });
        }

        let mut entre_blocs = |dest: &mut [Destination], lus: u64, debit: f64| {
            vider(&rx_retour, &mut etat);
            if dernier_avancement.elapsed() >= PERIODE_AVANCEMENT {
                dernier_avancement = Instant::now();
                let av = avancement(
                    lus,
                    total,
                    debit,
                    index,
                    nb,
                    &f.chemin,
                    dest,
                    horloge.elapsed(),
                    opts.mode,
                    None,
                );
                (etat.rappel)(Evenement::Avancement(av));
            }
            if dernier_diagnostic.elapsed() >= PERIODE_DIAGNOSTIC {
                dernier_diagnostic = Instant::now();
                let profil_source = ProfilDebit {
                    debit_moyen: debit,
                    duree_s: horloge.elapsed().as_secs_f64(),
                    ..Default::default()
                };
                let profils: Vec<ProfilDebit> = dest
                    .iter()
                    .map(|d| {
                        d.compteurs
                            .profil
                            .lock()
                            .map(|p| p.clone())
                            .unwrap_or_default()
                    })
                    .collect();
                let diag = diagnostic::analyser(&diagnostic::Entree {
                    source: diagnostic::Flux {
                        nom: opts.nom.clone(),
                        profil: &profil_source,
                        volume: volume_source.as_ref(),
                        attente_lecture_ms: 0,
                        temps_verif_ms: 0,
                    },
                    destinations: dest
                        .iter()
                        .enumerate()
                        .map(|(i, d)| diagnostic::Flux {
                            nom: noms[i].clone(),
                            profil: &profils[i],
                            volume: controle.volumes[i].as_ref(),
                            attente_lecture_ms: d.attente.ms(),
                            temps_verif_ms: 0,
                        })
                        .collect(),
                    nb_fichiers: nb,
                    taille_mediane,
                    octets_total: total,
                });
                if diag.a_quelque_chose_a_dire() {
                    (etat.rappel)(Evenement::Diagnostic(diag));
                }
            }
        };

        let resultat = lire_fichier(
            opts,
            &f.chemin,
            &reserve,
            &hacheurs,
            &mut destinations,
            &mut lus_total,
            &mut debit_source,
            &mut lecture,
            &mut attente_hachage,
            &mut entre_blocs,
        );

        let promesse = Promesse::new(algos.len());
        match &resultat {
            Ok(taille) => {
                promesse.fournir_lecture(Ok(*taille));
                etat.fichiers[index].taille = *taille;
            }
            Err(e) => {
                if e.code == Code::Annule {
                    annule = true;
                }
                promesse.fournir_lecture(Err(e.clone()));
                if e.code != Code::Annule {
                    etat.fichiers[index].erreur_source = Some(e.clone());
                    etat.noter(format!(
                        "ÉCHEC LECTURE  {} : {}",
                        f.chemin,
                        e.message(Langue::Fr)
                    ));
                    (etat.rappel)(Evenement::Source {
                        index,
                        chemin: f.chemin.clone(),
                        erreur: e.clone(),
                    });
                }
            }
        }
        for h in &hacheurs {
            let _ = h.send(VersHacheur::Fin(Arc::clone(&promesse)));
        }
        if annule {
            break;
        }
        for d in &destinations {
            let _ = d.tx.send(VersDestination::Fin {
                promesse: Arc::clone(&promesse),
                modifie: f.modifie,
                cree: f.cree,
            });
        }
        if resultat.is_ok() {
            promesses.push((index, promesse));
        }
    }

    // Plus rien à lire : les destinations finissent à leur rythme.
    for d in &destinations {
        let _ = d.tx.send(VersDestination::Fini);
    }
    drop(hacheurs);
    for h in fils_hachage {
        let _ = h.join();
    }
    // Empreintes de la carte, pour le rapport (les fils d'empreinte ont fini).
    for (index, p) in promesses.drain(..) {
        if let Ok((_, e)) = p.attendre(&Annulation::default()) {
            etat.fichiers[index].empreintes = e;
        }
    }
    loop {
        vider(&rx_retour, &mut etat);
        if destinations
            .iter()
            .all(|d| d.fil.as_ref().map(|f| f.is_finished()).unwrap_or(true))
        {
            break;
        }
        if dernier_avancement.elapsed() >= PERIODE_AVANCEMENT {
            dernier_avancement = Instant::now();
            let av = avancement(
                lus_total,
                total,
                debit_source.debit(),
                nb.saturating_sub(1),
                nb,
                "",
                &destinations,
                horloge.elapsed(),
                opts.mode,
                None,
            );
            (etat.rappel)(Evenement::Avancement(av));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let bilans: Vec<Bilan> = destinations
        .iter_mut()
        .map(|d| {
            d.fil
                .take()
                .and_then(|h| h.join().ok())
                .unwrap_or_else(|| Bilan {
                    erreur_fatale: Some(Erreur::new(
                        Code::Interne,
                        "fil de destination interrompu",
                    )),
                    ..Default::default()
                })
        })
        .collect();
    vider(&rx_retour, &mut etat);
    annule |= opts.annulation.demandee();

    // Toute copie sans verdict est un échec : jamais un silence.
    for f in etat.fichiers.iter_mut() {
        for (d, b) in bilans.iter().enumerate() {
            if f.copie(d).is_none() {
                let erreur = if let Some(e) = &f.erreur_source {
                    e.clone()
                } else if annule {
                    Erreur::new(Code::Annule, "")
                } else {
                    b.erreur_fatale
                        .clone()
                        .unwrap_or_else(|| Erreur::new(Code::DestinationPerdue, ""))
                };
                f.copies.push(Copie {
                    destination: d,
                    niveau: Niveau::Echec,
                    erreur: Some(erreur),
                    durabilite: None,
                    lecture: None,
                    residence: None,
                });
            }
        }
        f.copies.sort_by_key(|c| c.destination);
    }

    // Relecture de la carte.
    if opts.mode == ModeVerif::CompletCarte && !annule {
        (etat.rappel)(Evenement::Phase {
            phase: Phase::RelectureCarte,
        });
        let algo = hash::de_comparaison(&algos);
        let mut relus = 0u64;
        for i in 0..etat.fichiers.len() {
            if opts.annulation.demandee() {
                annule = true;
                break;
            }
            if etat.fichiers[i].erreur_source.is_some() {
                continue;
            }
            let chemin = etat.fichiers[i].chemin.clone();
            let attendu = etat.fichiers[i].empreinte(algo).map(str::to_string);
            let r = relire_carte(
                &joindre(&opts.source, &chemin),
                algo,
                &mut relus,
                &opts.annulation,
            );
            let identique = match (r, attendu) {
                (Ok(v), Some(a)) => v == a && !opts.fautes.source_instable(&chemin),
                _ => false,
            };
            etat.fichiers[i].relecture_carte = Some(identique);
            if !identique {
                etat.noter(format!("RELECTURE CARTE DIFFÉRENTE  {chemin}"));
            }
            if dernier_avancement.elapsed() >= PERIODE_AVANCEMENT {
                dernier_avancement = Instant::now();
                let av = avancement(
                    lus_total,
                    total,
                    0.0,
                    i,
                    nb,
                    &chemin,
                    &destinations,
                    horloge.elapsed(),
                    opts.mode,
                    Some(relus),
                );
                (etat.rappel)(Evenement::Avancement(av));
            }
        }
    }

    (etat.rappel)(Evenement::Phase {
        phase: Phase::Rapports,
    });
    let fin = Utc::now();

    let rapport_destinations: Vec<DestinationRapport> = opts
        .destinations
        .iter()
        .enumerate()
        .map(|(i, dossier)| {
            let b = &bilans[i];
            let ok = etat
                .fichiers
                .iter()
                .filter(|f| {
                    f.copie(i)
                        .map(|c| c.niveau > Niveau::Echec)
                        .unwrap_or(false)
                })
                .count();
            DestinationRapport {
                dossier: controle::absolu(dossier).display().to_string(),
                volume: controle.volumes[i].clone(),
                erreur_fatale: b.erreur_fatale.clone(),
                fichiers_ok: ok,
                fichiers_echec: etat.fichiers.len() - ok,
                octets_ecrits: b.octets_ecrits,
                profil: b.profil.clone(),
                temps_ecriture_ms: b.temps_ecriture_ms,
                temps_sync_ms: b.temps_sync_ms,
                temps_verif_ms: b.temps_verif_ms,
                attente_imputee_ms: destinations[i].attente.ms(),
                manifestes: b.manifestes.clone(),
                durabilite_degradee: b.durabilite_degradee,
            }
        })
        .collect();

    let mut rapport = Rapport {
        outil: OUTIL.into(),
        version: VERSION.into(),
        nom: opts.nom.clone(),
        source: controle::absolu(&opts.source).display().to_string(),
        volume_source,
        debut,
        fin,
        mode: opts.mode,
        algos: algos.clone(),
        algo_comparaison: hash::de_comparaison(&algos),
        operateur: opts.operateur.clone(),
        projet: opts.projet.clone(),
        machine,
        utilisateur,
        nb_fichiers: etat.fichiers.len(),
        octets: etat.fichiers.iter().map(|f| f.taille).sum(),
        nb_clips: inv.nb_clips(),
        dossiers: inv.dossiers.len(),
        dossiers_vides: inv.dossiers_vides().len(),
        exclus: inv.exclus.clone(),
        liens: inv.liens.clone(),
        illisibles: inv.illisibles.clone(),
        fichiers: std::mem::take(&mut etat.fichiers),
        destinations: rapport_destinations,
        annule,
        verdict: Verdict::Echec,
        remarques: Vec::new(),
        profil_source: debit_source.bilan(),
        temps_lecture_ms: lecture.ms(),
        diagnostic: None,
    };
    rapport.conclure(inv.segments_manquants());
    let diag = diagnostic::depuis_rapport(&rapport);
    if diag.a_quelque_chose_a_dire() {
        rapport.diagnostic = Some(diag);
    }

    etat.noter(format!(
        "Verdict : {} ({})",
        rapport.verdict.titre(Langue::Fr),
        crate::langue::duree(rapport.duree_s(), Langue::Fr)
    ));
    for r in &rapport.remarques {
        etat.noter(format!("Remarque : {}", r.texte(Langue::Fr, &noms)));
    }

    // Rapports sur chaque destination encore accessible.
    let json = rapport.json();
    let html = rapport_html::rendre(&rapport, opts.langue);
    let journal = etat.journal.join("\n") + "\n";
    for (i, d) in opts.destinations.iter().enumerate() {
        if !d.is_dir() {
            continue;
        }
        let dossier = d.join(DOSSIER_RAPPORT);
        let ecrit = (|| -> io::Result<()> {
            fs::create_dir_all(&dossier)?;
            for (nom, contenu) in [
                ("rapport.json", json.as_bytes()),
                ("rapport.html", html.as_bytes()),
                ("journal.log", journal.as_bytes()),
            ] {
                let mut f = File::create(dossier.join(nom))?;
                f.write_all(contenu)?;
                es::synchroniser(&f)?;
            }
            Ok(())
        })();
        if let Err(e) = ecrit {
            let x = Erreur::io(Code::EcritureImpossible, dossier.display(), &e);
            if rapport.destinations[i].erreur_fatale.is_none() {
                rapport.destinations[i].erreur_fatale = Some(x);
            }
        }
    }

    (etat.rappel)(Evenement::Phase {
        phase: Phase::Termine,
    });
    Ok(rapport)
}

fn relire_carte(
    chemin: &Path,
    algo: Algo,
    relus: &mut u64,
    annulation: &Annulation,
) -> Result<String, Erreur> {
    let mut l = sanscache::ouvrir(chemin, BLOC)
        .map_err(|e| Erreur::io(Code::LectureSource, chemin.display(), &e))?;
    let mut h = Hacheur::new(algo);
    loop {
        if annulation.demandee() {
            return Err(Erreur::new(Code::Annule, ""));
        }
        let b = l
            .lire_bloc()
            .map_err(|e| Erreur::io(Code::LectureSource, chemin.display(), &e))?;
        if b.is_empty() {
            break;
        }
        *relus += b.len() as u64;
        h.update(b);
    }
    Ok(h.finir())
}

#[allow(clippy::too_many_arguments)]
fn avancement(
    lus: u64,
    total: u64,
    debit_lecture: f64,
    index: usize,
    nb_fichiers: usize,
    chemin: &str,
    destinations: &[Destination],
    ecoule: Duration,
    mode: ModeVerif,
    relecture_carte: Option<u64>,
) -> Avancement {
    let dest: Vec<AvancementDestination> =
        destinations.iter().map(|d| d.compteurs.lire()).collect();
    let total = total.max(lus);
    // Fraction du travail faite par la destination la plus en retard :
    // écriture + relecture (sauf en mode taille seule).
    let passes = if mode == ModeVerif::Taille { 1.0 } else { 2.0 };
    let fraction = dest
        .iter()
        .filter(|d| !d.en_panne)
        .map(|d| {
            if d.termine {
                1.0
            } else {
                (d.ecrits + d.relus) as f64 / (total as f64 * passes).max(1.0)
            }
        })
        .fold(f64::INFINITY, f64::min);
    let e = ecoule.as_secs_f64();
    let restant = (fraction.is_finite() && fraction > 0.01 && fraction < 1.0 && e > 5.0)
        .then(|| e * (1.0 - fraction) / fraction);
    Avancement {
        lus,
        total,
        debit_lecture,
        index,
        nb_fichiers,
        chemin: chemin.to_string(),
        destinations: dest,
        ecoule_s: e,
        restant_s: restant,
        relecture_carte,
    }
}

/// Fichiers temporaires restés sur une destination : copies interrompues,
/// jamais des rushs exploitables.
pub fn residus(dossier: &Path) -> Vec<PathBuf> {
    walkdir::WalkDir::new(dossier)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .map(|e| e.into_path())
        .filter(|p| {
            p.file_name()
                .map(|n| n.to_string_lossy().ends_with(SUFFIXE_TEMP))
                .unwrap_or(false)
        })
        .collect()
}
