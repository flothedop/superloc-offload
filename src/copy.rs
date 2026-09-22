//! Moteur de copie : une seule lecture de la carte, plusieurs écritures.
//!
//! Déroulé pour chaque fichier :
//! 1. ouverture d'un fichier temporaire sur chaque destination ;
//! 2. lecture unique de la source, empreinte calculée à la volée, blocs
//!    envoyés en parallèle à toutes les destinations ;
//! 3. écriture forcée sur le support (`sync_all`) ;
//! 4. relecture de chaque destination en contournant le cache système ;
//! 5. renommage vers le nom définitif, uniquement si l'empreinte concorde.
//!
//! Un fichier interrompu garde son nom temporaire et ne peut donc jamais
//! passer pour un fichier complet.

use crate::hash::{Algo, MultiHasher};
use crate::report::{
    Empreinte, Etat, Rapport, RapportDestination, RapportFichier, VerifDestination, OUTIL,
};
use crate::scan::Scan;
use crate::{preflight, scan, verify, CHUNK};
use crossbeam_channel::{bounded, Receiver, Sender};
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread::JoinHandle;

/// Suffixe des fichiers en cours de copie.
pub const SUFFIXE_TEMP: &str = ".offload-part";

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
    Fichier {
        index: usize,
        total: usize,
        chemin: String,
    },
    Octets {
        copies: u64,
        total: u64,
    },
    Verification {
        chemin: String,
    },
    Resultat {
        index: usize,
        chemin: String,
        ok: bool,
        /// État de ce fichier sur chaque destination, dans l'ordre donné.
        verifications: Vec<VerifDestination>,
    },
}

enum Commande {
    Ouvrir(PathBuf),
    Bloc(Arc<Vec<u8>>),
    Cloturer(Sender<io::Result<()>>),
    Fin,
}

struct Ecrivain {
    dossier: PathBuf,
    tx: Sender<Commande>,
    handle: Option<JoinHandle<()>>,
    /// Une destination « morte » (disque débranché, plein…) est écartée des
    /// fichiers suivants mais reste consignée en échec dans le rapport.
    morte: bool,
    erreur: Option<String>,
    fichiers_ok: usize,
    fichiers_echec: usize,
    octets: u64,
}

fn boucle_ecrivain(rx: Receiver<Commande>) {
    let mut fichier: Option<File> = None;
    let mut erreur: Option<io::Error> = None;

    while let Ok(cmd) = rx.recv() {
        match cmd {
            Commande::Ouvrir(chemin) => {
                fichier = None;
                erreur = None;
                let r = (|| -> io::Result<File> {
                    if let Some(parent) = chemin.parent() {
                        fs::create_dir_all(parent)?;
                    }
                    // create_new : on n'écrase jamais un fichier existant.
                    fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(&chemin)
                })();
                match r {
                    Ok(f) => fichier = Some(f),
                    Err(e) => erreur = Some(e),
                }
            }
            Commande::Bloc(bloc) => {
                if erreur.is_some() {
                    continue;
                }
                if let Some(f) = fichier.as_mut() {
                    if let Err(e) = f.write_all(&bloc) {
                        erreur = Some(e);
                    }
                }
            }
            Commande::Cloturer(reponse) => {
                let r = (|| -> io::Result<()> {
                    if let Some(e) = erreur.take() {
                        return Err(e);
                    }
                    let mut f = fichier
                        .take()
                        .ok_or_else(|| io::Error::other("fichier de destination non ouvert"))?;
                    f.flush()?;
                    // Force l'écriture physique : sans cela, la relecture
                    // pourrait valider des données encore en mémoire.
                    f.sync_all()?;
                    Ok(())
                })();
                let _ = reponse.send(r);
            }
            Commande::Fin => break,
        }
    }
}

impl Ecrivain {
    fn nouveau(dossier: PathBuf) -> Self {
        // Canal borné : si un disque est lent, la lecture ralentit au lieu
        // de remplir la mémoire vive.
        let (tx, rx) = bounded::<Commande>(4);
        let handle = std::thread::spawn(move || boucle_ecrivain(rx));
        Ecrivain {
            dossier,
            tx,
            handle: Some(handle),
            morte: false,
            erreur: None,
            fichiers_ok: 0,
            fichiers_echec: 0,
            octets: 0,
        }
    }

    fn tuer(&mut self, motif: String) {
        if !self.morte {
            self.morte = true;
            self.erreur = Some(motif);
        }
    }

    fn arreter(&mut self) {
        let _ = self.tx.send(Commande::Fin);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
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

/// Exécute un déchargement complet : scan, contrôles, copie, vérification.
pub fn decharger(opts: &Options, progres: &mut dyn FnMut(Progression)) -> io::Result<Rapport> {
    let debut = chrono::Utc::now();
    let inventaire = scan::scanner(&opts.source)?;
    let pre = preflight::controler(&inventaire, &opts.destinations, &opts.source)?;

    progres(Progression::Demarrage {
        nb_fichiers: inventaire.fichiers.len(),
        octets_total: inventaire.octets_total,
    });

    let mut ecrivains: Vec<Ecrivain> = pre
        .destinations
        .iter()
        .map(|d| Ecrivain::nouveau(d.clone()))
        .collect();

    let mut fichiers_rapport: Vec<RapportFichier> = Vec::new();
    let mut avertissements = pre.avertissements.clone();
    let mut copies: u64 = 0;

    for (index, f) in inventaire.fichiers.iter().enumerate() {
        progres(Progression::Fichier {
            index: index + 1,
            total: inventaire.fichiers.len(),
            chemin: f.chemin.clone(),
        });

        let source_fichier = opts.source.join(&f.chemin);
        let mut cibles: Vec<(usize, PathBuf, PathBuf)> = Vec::new();
        // Une destination déjà tombée reste consignée en échec pour chaque
        // fichier suivant : sans cela, un fichier copié sur un seul disque
        // paraîtrait vérifié partout.
        let mut verifications: Vec<VerifDestination> = Vec::new();

        for (i, e) in ecrivains.iter_mut().enumerate() {
            if e.morte {
                e.fichiers_echec += 1;
                verifications.push(VerifDestination {
                    destination: i,
                    etat: Etat::Echec,
                    mode_lecture: None,
                    erreur: Some(
                        e.erreur
                            .clone()
                            .unwrap_or_else(|| "destination indisponible".to_string()),
                    ),
                });
                continue;
            }
            let final_ = e.dossier.join(&f.chemin);
            let temp = chemin_temporaire(&final_);
            if e.tx.send(Commande::Ouvrir(temp.clone())).is_err() {
                e.tuer("le fil d'écriture s'est arrêté".to_string());
                e.fichiers_echec += 1;
                verifications.push(VerifDestination {
                    destination: i,
                    etat: Etat::Echec,
                    mode_lecture: None,
                    erreur: Some("le fil d'écriture s'est arrêté".to_string()),
                });
                continue;
            }
            cibles.push((i, temp, final_));
        }

        if cibles.is_empty() {
            return Err(io::Error::other(
                "plus aucune destination disponible : copie interrompue",
            ));
        }

        // --- Lecture unique de la source, diffusée à toutes les destinations.
        let mut hasher = MultiHasher::new(&opts.algos);
        let mut source = File::open(&source_fichier)?;
        let mut tampon = vec![0u8; CHUNK];
        let mut erreur_lecture: Option<io::Error> = None;

        loop {
            let n = match source.read(&mut tampon) {
                Ok(0) => break,
                Ok(n) => n,
                Err(ref e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => {
                    erreur_lecture = Some(e);
                    break;
                }
            };
            hasher.update(&tampon[..n]);
            let bloc = Arc::new(tampon[..n].to_vec());
            for (i, _, _) in cibles.iter() {
                let e = &mut ecrivains[*i];
                if e.morte {
                    continue;
                }
                if e.tx.send(Commande::Bloc(Arc::clone(&bloc))).is_err() {
                    e.tuer("le fil d'écriture s'est arrêté".to_string());
                }
            }
            copies += n as u64;
            progres(Progression::Octets {
                copies,
                total: inventaire.octets_total,
            });
        }

        let octets_lus = hasher.bytes();
        let empreintes: Vec<Empreinte> = hasher
            .finish()
            .into_iter()
            .map(|(algo, valeur)| Empreinte { algo, valeur })
            .collect();

        // --- Clôture : vidage des tampons et écriture physique.
        for (i, temp, final_) in cibles.iter() {
            let (tx_rep, rx_rep) = bounded(1);
            let ferme = if ecrivains[*i].morte {
                Err(io::Error::other(
                    ecrivains[*i]
                        .erreur
                        .clone()
                        .unwrap_or_else(|| "destination indisponible".to_string()),
                ))
            } else if ecrivains[*i].tx.send(Commande::Cloturer(tx_rep)).is_err() {
                Err(io::Error::other("le fil d'écriture s'est arrêté"))
            } else {
                rx_rep
                    .recv()
                    .unwrap_or_else(|_| Err(io::Error::other("pas de réponse du fil d'écriture")))
            };

            if let Some(e) = &erreur_lecture {
                // Source illisible : on ne garde surtout pas la copie partielle.
                let _ = fs::remove_file(temp);
                ecrivains[*i].fichiers_echec += 1;
                verifications.push(VerifDestination {
                    destination: *i,
                    etat: Etat::Echec,
                    mode_lecture: None,
                    erreur: Some(format!("lecture de la source impossible : {e}")),
                });
                continue;
            }

            if let Err(e) = ferme {
                let _ = fs::remove_file(temp);
                ecrivains[*i].fichiers_echec += 1;
                let motif = format!("écriture impossible : {e}");
                ecrivains[*i].tuer(motif.clone());
                verifications.push(VerifDestination {
                    destination: *i,
                    etat: Etat::Echec,
                    mode_lecture: None,
                    erreur: Some(motif),
                });
                continue;
            }

            // --- Relecture depuis le support, sans cache.
            progres(Progression::Verification {
                chemin: f.chemin.clone(),
            });

            match verify::relire(temp, &opts.algos) {
                Ok(relu) => {
                    if !relu.mode.fiable() {
                        let a = format!(
                            "{} : le système n'a pas permis de contourner le cache, la vérification est dégradée",
                            ecrivains[*i].dossier.display()
                        );
                        if !avertissements.contains(&a) {
                            avertissements.push(a);
                        }
                    }
                    match verify::comparer(&empreintes, octets_lus, &relu) {
                        Ok(()) => match valider(temp, final_) {
                            Ok(()) => {
                                ecrivains[*i].fichiers_ok += 1;
                                ecrivains[*i].octets += octets_lus;
                                verifications.push(VerifDestination {
                                    destination: *i,
                                    etat: Etat::Ok,
                                    mode_lecture: Some(relu.mode),
                                    erreur: None,
                                });
                            }
                            Err(e) => {
                                ecrivains[*i].fichiers_echec += 1;
                                verifications.push(VerifDestination {
                                    destination: *i,
                                    etat: Etat::Echec,
                                    mode_lecture: Some(relu.mode),
                                    erreur: Some(format!("renommage impossible : {e}")),
                                });
                            }
                        },
                        Err(detail) => {
                            let _ = fs::remove_file(temp);
                            ecrivains[*i].fichiers_echec += 1;
                            verifications.push(VerifDestination {
                                destination: *i,
                                etat: Etat::Echec,
                                mode_lecture: Some(relu.mode),
                                erreur: Some(detail),
                            });
                        }
                    }
                }
                Err(e) => {
                    let _ = fs::remove_file(temp);
                    ecrivains[*i].fichiers_echec += 1;
                    verifications.push(VerifDestination {
                        destination: *i,
                        etat: Etat::Echec,
                        mode_lecture: None,
                        erreur: Some(format!("relecture impossible : {e}")),
                    });
                }
            }
        }

        if let Some(e) = erreur_lecture {
            return Err(io::Error::new(
                e.kind(),
                format!("lecture de {} impossible : {e}", f.chemin),
            ));
        }

        let tout_ok = verifications.iter().all(|v| v.etat.ok());
        verifications.sort_by_key(|v| v.destination);
        progres(Progression::Resultat {
            index,
            chemin: f.chemin.clone(),
            ok: tout_ok,
            verifications: verifications.clone(),
        });

        fichiers_rapport.push(RapportFichier {
            chemin: f.chemin.clone(),
            clip: f.clip.clone(),
            taille: octets_lus,
            modifie_le: f.modifie_le.clone(),
            empreintes,
            verifications,
        });
    }

    for e in ecrivains.iter_mut() {
        e.arreter();
    }

    // --- Relecture optionnelle de la carte source.
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

    let destinations: Vec<RapportDestination> = ecrivains
        .iter()
        .map(|e| RapportDestination {
            dossier: e.dossier.display().to_string(),
            etat: if e.erreur.is_none()
                && e.fichiers_echec == 0
                && e.fichiers_ok == inventaire.fichiers.len()
            {
                Etat::Ok
            } else {
                Etat::Echec
            },
            erreur: e.erreur.clone(),
            fichiers_ok: e.fichiers_ok,
            fichiers_echec: e.fichiers_echec,
            octets_ecrits: e.octets,
            mhl: None,
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
        destinations,
        source_reverifiee,
        etat_global: Etat::Echec,
        avertissements,
        machine: whoami::fallible::hostname().unwrap_or_else(|_| "inconnu".to_string()),
        utilisateur: whoami::username(),
    };
    rapport.recalculer_etat();

    Ok(rapport)
}

fn reverifier_source(
    source: &Path,
    fichiers: &[RapportFichier],
    algos: &[Algo],
    avertissements: &mut Vec<String>,
) -> Etat {
    let mut etat = Etat::Ok;
    for f in fichiers {
        let chemin = source.join(&f.chemin);
        match verify::relire(&chemin, algos) {
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

fn chemin_temporaire(final_: &Path) -> PathBuf {
    let mut nom = final_.file_name().unwrap_or_default().to_os_string();
    nom.push(SUFFIXE_TEMP);
    final_.with_file_name(nom)
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
