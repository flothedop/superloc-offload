//! Moteur de copie : une seule lecture de la carte, plusieurs écritures.
//!
//! Chaque destination possède son propre fil d'exécution et avance à son
//! rythme : elle écrit, force l'écriture physique, relit le fichier depuis
//! le support et le renomme sans attendre les autres. Un disque rapide
//! n'attend donc plus la vérification d'un disque lent.
//!
//! Le découplage n'est pas total, et ne peut pas l'être : la carte n'étant
//! lue qu'une fois, les blocs doivent être conservés en mémoire tant que la
//! destination la plus lente ne les a pas consommés. Le tampon est donc
//! borné — les blocs sont partagés entre destinations, si bien que l'écart
//! coûte de la mémoire une seule fois et non une fois par disque.

use crate::hash::{Algo, MultiHasher};
use crate::mesure::{Cumul, Debitmetre, ProfilDebit};
use crate::report::{
    Empreinte, Etat, Rapport, RapportDestination, RapportFichier, VerifDestination, OUTIL,
};
use crate::scan::Scan;
use crate::uncached::Mode;
use crate::{preflight, scan, verify, volume, CHUNK};
use crossbeam_channel::{bounded, Receiver, Sender, TrySendError};
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// Suffixe des fichiers en cours de copie.
pub const SUFFIXE_TEMP: &str = ".offload-part";

/// Nombre de blocs qu'une destination peut avoir en retard. Les blocs étant
/// partagés, 32 blocs de 4 Mio représentent au plus 128 Mio de mémoire au
/// total, quel que soit le nombre de disques.
const TAMPON_BLOCS: usize = 32;

/// Cadence des remontées de débit vers l'interface.
const PERIODE_DEBIT: Duration = Duration::from_millis(400);
/// Cadence d'envoi du profil complet, qui alimente le diagnostic en direct.
const PERIODE_PROFIL: Duration = Duration::from_secs(5);
/// Cadence de recalcul du diagnostic pendant la copie.
const PERIODE_DIAGNOSTIC: Duration = Duration::from_secs(15);

pub struct Options {
    pub source: PathBuf,
    /// Dossier cible complet sur chaque destination (ne doit pas exister).
    pub destinations: Vec<PathBuf>,
    pub algos: Vec<Algo>,
    /// Relire aussi la carte source après copie, pour détecter un lecteur
    /// de cartes défaillant.
    pub verifier_source: bool,
    /// Nom du déchargement (carte, volume) repris dans les rapports.
    pub nom: String,
}

#[derive(Debug)]
pub enum Progression {
    Demarrage {
        nb_fichiers: usize,
        octets_total: u64,
    },
    /// La lecture de la source attaque ce fichier.
    Fichier {
        index: usize,
        total: usize,
        chemin: String,
    },
    /// Avancement de la lecture de la carte.
    Octets { copies: u64, total: u64, debit: f64 },
    /// Débit d'écriture d'une destination, mesuré sur le temps réel.
    Debit {
        destination: usize,
        debit: f64,
        cumul: u64,
        fichiers_faits: usize,
    },
    /// Analyse de la lenteur, recalculée pendant la copie.
    Diagnostic(crate::diagnostic::Diagnostic),
    /// Une destination vient de vérifier — ou de rater — un fichier.
    Verifie {
        destination: usize,
        index: usize,
        chemin: String,
        etat: Etat,
        erreur: Option<String>,
    },
}

// ---------------------------------------------------------------------------
// Dialogue avec les fils de destination
// ---------------------------------------------------------------------------

enum Message {
    Debut {
        index: usize,
        relatif: String,
    },
    Bloc(Arc<Vec<u8>>),
    Fin {
        taille: u64,
        empreintes: Vec<Empreinte>,
    },
    Stop,
}

enum Retour {
    Debit {
        destination: usize,
        debit: f64,
        cumul: u64,
        fichiers_faits: usize,
        /// Envoyé périodiquement seulement : alimente le diagnostic.
        profil: Option<ProfilDebit>,
    },
    Verifie {
        destination: usize,
        index: usize,
        chemin: String,
        etat: Etat,
        erreur: Option<String>,
        mode: Option<Mode>,
    },
}

/// Ce qu'une destination rapporte en fin de course.
struct Bilan {
    fichiers_ok: usize,
    fichiers_echec: usize,
    octets: u64,
    erreur_fatale: Option<String>,
    profil: ProfilDebit,
    temps_ecriture_ms: u64,
    temps_sync_ms: u64,
    temps_verif_ms: u64,
}

struct FichierEnCours {
    index: usize,
    relatif: String,
    temp: PathBuf,
    final_: PathBuf,
    fichier: Option<File>,
    erreur: Option<String>,
}

fn boucle_destination(
    index_dest: usize,
    dossier: PathBuf,
    algos: Vec<Algo>,
    rx: Receiver<Message>,
    tx: Sender<Retour>,
) -> Bilan {
    let mut debitmetre = Debitmetre::new();
    let mut ecriture = Cumul::default();
    let mut sync = Cumul::default();
    let mut verif = Cumul::default();
    let mut courant: Option<FichierEnCours> = None;
    let mut fichiers_ok = 0usize;
    let mut fichiers_echec = 0usize;
    let mut erreur_fatale: Option<String> = None;
    let mut derniere_remontee = Instant::now();
    let mut dernier_profil = Instant::now();

    while let Ok(message) = rx.recv() {
        match message {
            Message::Stop => break,

            Message::Debut { index, relatif } => {
                let final_ = dossier.join(&relatif);
                let temp = chemin_temporaire(&final_);
                let mut etat = FichierEnCours {
                    index,
                    relatif,
                    temp: temp.clone(),
                    final_,
                    fichier: None,
                    erreur: erreur_fatale.clone(),
                };
                if etat.erreur.is_none() {
                    let ouverture = (|| -> io::Result<File> {
                        if let Some(parent) = temp.parent() {
                            fs::create_dir_all(parent)?;
                        }
                        // create_new : on n'écrase jamais un fichier existant.
                        fs::OpenOptions::new()
                            .write(true)
                            .create_new(true)
                            .open(&temp)
                    })();
                    match ouverture {
                        Ok(f) => etat.fichier = Some(f),
                        Err(e) => {
                            let motif = format!("écriture impossible : {e}");
                            etat.erreur = Some(motif.clone());
                            erreur_fatale.get_or_insert(motif);
                        }
                    }
                }
                courant = Some(etat);
            }

            Message::Bloc(bloc) => {
                let Some(etat) = courant.as_mut() else {
                    continue;
                };
                if etat.erreur.is_some() {
                    continue;
                }
                if let Some(f) = etat.fichier.as_mut() {
                    let taille = bloc.len() as u64;
                    let r = ecriture.mesurer(|| f.write_all(&bloc));
                    match r {
                        Ok(()) => {
                            debitmetre.ajouter(taille);
                            if derniere_remontee.elapsed() >= PERIODE_DEBIT {
                                derniere_remontee = Instant::now();
                                let profil = if dernier_profil.elapsed() >= PERIODE_PROFIL {
                                    dernier_profil = Instant::now();
                                    Some(debitmetre.bilan())
                                } else {
                                    None
                                };
                                let _ = tx.send(Retour::Debit {
                                    destination: index_dest,
                                    debit: debitmetre.debit(),
                                    cumul: debitmetre.cumul(),
                                    fichiers_faits: fichiers_ok,
                                    profil,
                                });
                            }
                        }
                        Err(e) => {
                            let motif = format!("écriture impossible : {e}");
                            etat.erreur = Some(motif.clone());
                            erreur_fatale.get_or_insert(motif);
                        }
                    }
                }
            }

            Message::Fin { taille, empreintes } => {
                let Some(mut etat) = courant.take() else {
                    continue;
                };

                // Écriture physique forcée : sans cela, la relecture
                // validerait des données encore en mémoire vive.
                if etat.erreur.is_none() {
                    if let Some(f) = etat.fichier.as_mut() {
                        let r = sync.mesurer(|| f.flush().and_then(|()| f.sync_all()));
                        if let Err(e) = r {
                            let motif = format!("écriture physique impossible : {e}");
                            etat.erreur = Some(motif.clone());
                            erreur_fatale.get_or_insert(motif);
                        }
                    }
                }
                etat.fichier = None;

                let (resultat, mode) = if let Some(motif) = etat.erreur.clone() {
                    let _ = fs::remove_file(&etat.temp);
                    (Err(motif), None)
                } else {
                    // Relecture depuis le support, sans passer par le cache.
                    let relu = verif.mesurer(|| verify::relire(&etat.temp, &algos));
                    match relu {
                        Ok(relu) => {
                            let mode = relu.mode;
                            match verify::comparer(&empreintes, taille, &relu) {
                                Ok(()) => match valider(&etat.temp, &etat.final_) {
                                    Ok(()) => (Ok(()), Some(mode)),
                                    Err(e) => {
                                        (Err(format!("renommage impossible : {e}")), Some(mode))
                                    }
                                },
                                Err(detail) => {
                                    let _ = fs::remove_file(&etat.temp);
                                    (Err(detail), Some(mode))
                                }
                            }
                        }
                        Err(e) => {
                            let _ = fs::remove_file(&etat.temp);
                            (Err(format!("relecture impossible : {e}")), None)
                        }
                    }
                };

                let (etat_fichier, erreur) = match resultat {
                    Ok(()) => {
                        fichiers_ok += 1;
                        (Etat::Ok, None)
                    }
                    Err(motif) => {
                        fichiers_echec += 1;
                        (Etat::Echec, Some(motif))
                    }
                };

                let _ = tx.send(Retour::Verifie {
                    destination: index_dest,
                    index: etat.index,
                    chemin: etat.relatif.clone(),
                    etat: etat_fichier,
                    erreur,
                    mode,
                });
                let _ = tx.send(Retour::Debit {
                    destination: index_dest,
                    debit: debitmetre.debit(),
                    cumul: debitmetre.cumul(),
                    fichiers_faits: fichiers_ok,
                    profil: Some(debitmetre.bilan()),
                });
            }
        }
    }

    Bilan {
        fichiers_ok,
        fichiers_echec,
        octets: debitmetre.cumul(),
        erreur_fatale,
        profil: debitmetre.bilan(),
        temps_ecriture_ms: ecriture.ms(),
        temps_sync_ms: sync.ms(),
        temps_verif_ms: verif.ms(),
    }
}

struct Destination {
    dossier: PathBuf,
    tx: Sender<Message>,
    handle: Option<JoinHandle<Bilan>>,
    morte: bool,
    attente: Cumul,
}

/// Renomme le fichier temporaire vers son nom définitif.
fn valider(temp: &Path, final_: &Path) -> io::Result<()> {
    if final_.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{} existe déjà", final_.display()),
        ));
    }
    fs::rename(temp, final_)
}

fn chemin_temporaire(final_: &Path) -> PathBuf {
    let mut nom = final_.file_name().unwrap_or_default().to_os_string();
    nom.push(SUFFIXE_TEMP);
    final_.with_file_name(nom)
}

/// Exécute un déchargement complet : scan, contrôles, copie, vérification.
pub fn decharger(opts: &Options, progres: &mut dyn FnMut(Progression)) -> io::Result<Rapport> {
    let debut = chrono::Utc::now();
    let inventaire = scan::scanner(&opts.source)?;
    let pre = preflight::controler(&inventaire, &opts.destinations, &opts.source)?;

    progres(Progression::Demarrage {
        nb_fichiers: inventaire.fichiers.len(),
        octets_total: inventaire.octets_total,
    });

    let (tx_retour, rx_retour) = bounded::<Retour>(256);
    let mut destinations: Vec<Destination> = pre
        .destinations
        .iter()
        .enumerate()
        .map(|(i, dossier)| {
            let (tx, rx) = bounded::<Message>(TAMPON_BLOCS);
            let dossier_fil = dossier.clone();
            let algos = opts.algos.clone();
            let retour = tx_retour.clone();
            let handle =
                std::thread::spawn(move || boucle_destination(i, dossier_fil, algos, rx, retour));
            Destination {
                dossier: dossier.clone(),
                tx,
                handle: Some(handle),
                morte: false,
                attente: Cumul::default(),
            }
        })
        .collect();
    drop(tx_retour);

    // Chaque fichier du rapport est préparé dès le scan ; les vérifications
    // s'y ajoutent au fur et à mesure que les destinations les remontent.
    let mut fichiers_rapport: Vec<RapportFichier> = inventaire
        .fichiers
        .iter()
        .map(|f| RapportFichier {
            chemin: f.chemin.clone(),
            clip: f.clip.clone(),
            taille: f.taille,
            modifie_le: f.modifie_le.clone(),
            empreintes: Vec::new(),
            verifications: Vec::new(),
        })
        .collect();

    let mut avertissements = pre.avertissements.clone();
    let mut modes: Vec<Option<Mode>> = vec![None; destinations.len()];
    // Dernier profil connu de chaque destination : sert au diagnostic
    // recalculé pendant la copie.
    let mut profils: Vec<ProfilDebit> = vec![ProfilDebit::default(); destinations.len()];
    let mut dernier_diagnostic = Instant::now();
    let mut copies: u64 = 0;
    let mut lecture = Cumul::default();
    let mut debit_source = Debitmetre::new();

    // Relevés faits une seule fois : ils ne changent pas pendant la copie.
    let volumes: Vec<Option<volume::Volume>> = destinations
        .iter()
        .map(|d| volume::du_chemin(&d.dossier))
        .collect();
    let volume_source = volume::du_chemin(&opts.source);
    let noms_destinations: Vec<String> = destinations
        .iter()
        .map(|d| {
            d.dossier
                .parent()
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| d.dossier.display().to_string())
        })
        .collect();
    let taille_mediane = {
        let mut t: Vec<u64> = inventaire.fichiers.iter().map(|f| f.taille).collect();
        t.sort_unstable();
        t.get(t.len() / 2).copied().unwrap_or(0)
    };

    for (index, f) in inventaire.fichiers.iter().enumerate() {
        progres(Progression::Fichier {
            index,
            total: inventaire.fichiers.len(),
            chemin: f.chemin.clone(),
        });

        for d in destinations.iter_mut() {
            if d.morte {
                continue;
            }
            let message = Message::Debut {
                index,
                relatif: f.chemin.clone(),
            };
            if d.tx.send(message).is_err() {
                d.morte = true;
            }
        }
        if destinations.iter().all(|d| d.morte) {
            return Err(io::Error::other(
                "plus aucune destination disponible : copie interrompue",
            ));
        }

        let mut hasher = MultiHasher::new(&opts.algos);
        let mut source = File::open(opts.source.join(&f.chemin))?;
        let mut tampon = vec![0u8; CHUNK];

        loop {
            let lu = lecture.mesurer(|| source.read(&mut tampon));
            let n = match lu {
                Ok(0) => break,
                Ok(n) => n,
                Err(ref e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => {
                    return Err(io::Error::new(
                        e.kind(),
                        format!("lecture de {} impossible : {e}", f.chemin),
                    ))
                }
            };
            hasher.update(&tampon[..n]);
            debit_source.ajouter(n as u64);

            let bloc = Arc::new(tampon[..n].to_vec());
            for d in destinations.iter_mut() {
                if d.morte {
                    continue;
                }
                envoyer(d, Message::Bloc(Arc::clone(&bloc)), &rx_retour, &mut |r| {
                    appliquer(r, &mut fichiers_rapport, &mut modes, &mut profils, progres)
                });
            }

            copies += n as u64;
            progres(Progression::Octets {
                copies,
                total: inventaire.octets_total,
                debit: debit_source.debit(),
            });
            vider(&rx_retour, &mut |r| {
                appliquer(r, &mut fichiers_rapport, &mut modes, &mut profils, progres)
            });

            if dernier_diagnostic.elapsed() >= PERIODE_DIAGNOSTIC {
                dernier_diagnostic = Instant::now();
                let profil_source = debit_source.bilan();
                let attentes: Vec<u64> = destinations.iter().map(|d| d.attente.ms()).collect();
                let diag = crate::diagnostic::analyser(&crate::diagnostic::Entree {
                    source: crate::diagnostic::Flux {
                        nom: opts.nom.clone(),
                        profil: &profil_source,
                        volume: volume_source.as_ref(),
                        attente_lecture_ms: 0,
                        temps_verif_ms: 0,
                    },
                    destinations: noms_destinations
                        .iter()
                        .enumerate()
                        .map(|(i, nom)| crate::diagnostic::Flux {
                            nom: nom.clone(),
                            profil: &profils[i],
                            volume: volumes[i].as_ref(),
                            attente_lecture_ms: attentes[i],
                            temps_verif_ms: 0,
                        })
                        .collect(),
                    nb_fichiers: inventaire.fichiers.len(),
                    taille_mediane,
                    octets_total: inventaire.octets_total,
                });
                if diag.a_quelque_chose_a_dire() {
                    progres(Progression::Diagnostic(diag));
                }
            }
        }

        let taille_lue = hasher.bytes();
        let empreintes: Vec<Empreinte> = hasher
            .finish()
            .into_iter()
            .map(|(algo, valeur)| Empreinte { algo, valeur })
            .collect();
        fichiers_rapport[index].taille = taille_lue;
        fichiers_rapport[index].empreintes = empreintes.clone();

        for d in destinations.iter_mut() {
            if d.morte {
                continue;
            }
            let message = Message::Fin {
                taille: taille_lue,
                empreintes: empreintes.clone(),
            };
            envoyer(d, message, &rx_retour, &mut |r| {
                appliquer(r, &mut fichiers_rapport, &mut modes, &mut profils, progres)
            });
        }
    }

    // Les destinations terminent leur file d'attente à leur rythme.
    for d in destinations.iter_mut() {
        let _ = d.tx.send(Message::Stop);
    }
    let mut bilans: Vec<Option<Bilan>> = (0..destinations.len()).map(|_| None).collect();
    for (i, d) in destinations.iter_mut().enumerate() {
        if let Some(h) = d.handle.take() {
            // On continue de relayer l'avancement pendant que les
            // destinations finissent de vérifier.
            while !h.is_finished() {
                vider(&rx_retour, &mut |r| {
                    appliquer(r, &mut fichiers_rapport, &mut modes, &mut profils, progres)
                });
                std::thread::sleep(Duration::from_millis(20));
            }
            bilans[i] = h.join().ok();
        }
    }
    vider(&rx_retour, &mut |r| {
        appliquer(r, &mut fichiers_rapport, &mut modes, &mut profils, progres)
    });

    // Un fichier sans verdict sur une destination est un échec : il ne doit
    // jamais passer pour vérifié.
    for (i, bilan) in bilans.iter().enumerate() {
        let motif = bilan
            .as_ref()
            .and_then(|b| b.erreur_fatale.clone())
            .unwrap_or_else(|| "destination indisponible".to_string());
        for fichier in fichiers_rapport.iter_mut() {
            if !fichier.verifications.iter().any(|v| v.destination == i) {
                fichier.verifications.push(VerifDestination {
                    destination: i,
                    etat: Etat::Echec,
                    mode_lecture: None,
                    erreur: Some(motif.clone()),
                });
            }
        }
    }
    for fichier in fichiers_rapport.iter_mut() {
        fichier.verifications.sort_by_key(|v| v.destination);
    }

    for (i, m) in modes.iter().enumerate() {
        if matches!(m, Some(mode) if !mode.fiable()) {
            avertissements.push(format!(
                "{} : le système n'a pas permis de contourner le cache, la vérification est dégradée",
                destinations[i].dossier.display()
            ));
        }
    }

    let source_reverifiee = if opts.verifier_source {
        Some(reverifier_source(
            &opts.source,
            &fichiers_rapport,
            &opts.algos,
            &mut avertissements,
        ))
    } else {
        None
    };

    let rapport_destinations: Vec<RapportDestination> = destinations
        .iter()
        .enumerate()
        .map(|(i, d)| {
            let bilan = bilans[i].as_ref();
            let ok = bilan.map(|b| b.fichiers_ok).unwrap_or(0);
            let echec = bilan.map(|b| b.fichiers_echec).unwrap_or(0);
            RapportDestination {
                dossier: d.dossier.display().to_string(),
                etat: if bilan.map(|b| b.erreur_fatale.is_none()).unwrap_or(false)
                    && echec == 0
                    && ok == inventaire.fichiers.len()
                {
                    Etat::Ok
                } else {
                    Etat::Echec
                },
                erreur: bilan.and_then(|b| b.erreur_fatale.clone()),
                fichiers_ok: ok,
                fichiers_echec: echec,
                octets_ecrits: bilan.map(|b| b.octets).unwrap_or(0),
                mhl: None,
                volume: volumes[i].clone(),
                profil: bilan.map(|b| b.profil.clone()).unwrap_or_default(),
                temps_ecriture_ms: bilan.map(|b| b.temps_ecriture_ms).unwrap_or(0),
                temps_sync_ms: bilan.map(|b| b.temps_sync_ms).unwrap_or(0),
                temps_verif_ms: bilan.map(|b| b.temps_verif_ms).unwrap_or(0),
                attente_lecture_ms: d.attente.ms(),
            }
        })
        .collect();

    let mut rapport = Rapport {
        outil: OUTIL.to_string(),
        nom: opts.nom.clone(),
        source: opts.source.display().to_string(),
        debut: debut.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        fin: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        algos: opts.algos.clone(),
        octets_total: inventaire.octets_total,
        nb_fichiers: inventaire.fichiers.len(),
        nb_clips: inventaire.nb_clips(),
        ignores: inventaire.ignores.clone(),
        liens: inventaire.liens.clone(),
        fichiers: fichiers_rapport,
        destinations: rapport_destinations,
        source_reverifiee,
        etat_global: Etat::Echec,
        avertissements,
        machine: whoami::fallible::hostname().unwrap_or_else(|_| "inconnu".to_string()),
        utilisateur: whoami::username(),
        volume_source,
        profil_source: debit_source.bilan(),
        temps_lecture_ms: lecture.ms(),
    };
    rapport.recalculer_etat();

    Ok(rapport)
}

/// Envoie un message à une destination sans jamais bloquer indéfiniment,
/// en relayant l'avancement pendant l'attente et en imputant le temps perdu
/// à cette destination — c'est cette mesure qui désigne le goulot.
fn envoyer(
    d: &mut Destination,
    mut message: Message,
    rx: &Receiver<Retour>,
    relayer: &mut dyn FnMut(Retour),
) {
    let debut = Instant::now();
    let mut a_attendu = false;
    loop {
        match d.tx.try_send(message) {
            Ok(()) => break,
            Err(TrySendError::Full(m)) => {
                a_attendu = true;
                message = m;
                vider(rx, relayer);
                std::thread::sleep(Duration::from_millis(2));
            }
            Err(TrySendError::Disconnected(_)) => {
                d.morte = true;
                break;
            }
        }
    }
    if a_attendu {
        d.attente.ajouter(debut.elapsed());
    }
}

fn vider(rx: &Receiver<Retour>, relayer: &mut dyn FnMut(Retour)) {
    while let Ok(r) = rx.try_recv() {
        relayer(r);
    }
}

fn appliquer(
    retour: Retour,
    fichiers: &mut [RapportFichier],
    modes: &mut [Option<Mode>],
    profils: &mut [ProfilDebit],
    progres: &mut dyn FnMut(Progression),
) {
    match retour {
        Retour::Debit {
            destination,
            debit,
            cumul,
            fichiers_faits,
            profil,
        } => {
            if let (Some(p), Some(slot)) = (profil, profils.get_mut(destination)) {
                *slot = p;
            }
            progres(Progression::Debit {
                destination,
                debit,
                cumul,
                fichiers_faits,
            })
        }
        Retour::Verifie {
            destination,
            index,
            chemin,
            etat,
            erreur,
            mode,
        } => {
            if let Some(f) = fichiers.get_mut(index) {
                f.verifications.push(VerifDestination {
                    destination,
                    etat,
                    mode_lecture: mode,
                    erreur: erreur.clone(),
                });
            }
            if let Some(slot) = modes.get_mut(destination) {
                if slot.is_none() {
                    *slot = mode;
                }
            }
            progres(Progression::Verifie {
                destination,
                index,
                chemin,
                etat,
                erreur,
            });
        }
    }
}

fn reverifier_source(
    source: &Path,
    fichiers: &[RapportFichier],
    algos: &[Algo],
    avertissements: &mut Vec<String>,
) -> Etat {
    let mut etat = Etat::Ok;
    for f in fichiers {
        match verify::relire(&source.join(&f.chemin), algos) {
            Ok(relu) => {
                if let Err(detail) = verify::comparer(&f.empreintes, f.taille, &relu) {
                    avertissements.push(format!(
                        "relecture de la source différente pour {} : {detail}",
                        f.chemin
                    ));
                    etat = Etat::Echec;
                }
            }
            Err(e) => {
                avertissements.push(format!(
                    "relecture de la source impossible pour {} : {e}",
                    f.chemin
                ));
                etat = Etat::Echec;
            }
        }
    }
    etat
}

/// Recense les fichiers temporaires restés sur une destination : ce sont des
/// copies interrompues, jamais des rushs exploitables.
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

pub fn inventaire_source(source: &Path) -> io::Result<Scan> {
    scan::scanner(source)
}
