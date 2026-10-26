//! Tests d'intégrité de bout en bout.
//!
//! Les pannes sont injectées PENDANT la copie (bit retourné à l'écriture,
//! disque plein, disque débranché, écriture perdue, secteur illisible sur la
//! carte, carte modifiée pendant la lecture, annulation), et non en
//! corrompant une copie après coup.

use offload_core::erreur::Code;
use offload_core::faute::{Panne, Plan};
use offload_core::hash::Algo;
use offload_core::moteur::{decharger, residus, Evenement, Options};
use offload_core::rapport::{ModeVerif, Niveau, Rapport, Remarque, Verdict};
use offload_core::{ascmhl, verification};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

/// Dossier de travail sur un vrai disque (pas un tmpfs).
fn base() -> tempfile::TempDir {
    let b = Path::new(env!("CARGO_TARGET_TMPDIR")).join("integrite");
    fs::create_dir_all(&b).unwrap();
    tempfile::tempdir_in(&b).unwrap()
}

fn donnees(n: usize, graine: u8) -> Vec<u8> {
    (0..n)
        .map(|i| (i as u8).wrapping_mul(31).wrapping_add(graine))
        .collect()
}

const DATE_CARTE: u64 = 1_760_000_000;

/// Une carte RED plausible : segments R3D, proxy, fichier de 0 octet,
/// dossier vide, fichiers système à exclure, nom accentué.
fn carte(racine: &Path) -> PathBuf {
    let c = racine.join("A001");
    let rdc = c.join("A001_0921XY.RDM/A001_C001_0921XY.RDC");
    fs::create_dir_all(&rdc).unwrap();
    fs::write(
        rdc.join("A001_C001_0921XY_001.R3D"),
        donnees(20 * 1024 * 1024 + 4097, 1),
    )
    .unwrap();
    fs::write(
        rdc.join("A001_C001_0921XY_002.R3D"),
        donnees(3 * 1024 * 1024 + 1, 2),
    )
    .unwrap();
    fs::write(rdc.join("A001_C001_0921XY.mov"), donnees(700_000, 3)).unwrap();
    fs::write(c.join("A001_0921XY.RDM/vide.txt"), b"").unwrap();
    fs::create_dir_all(c.join("A001_0921XY.RDM/VIDE")).unwrap();
    fs::write(c.join("Scène 1 — prise 2.wav"), donnees(1234, 4)).unwrap();
    fs::create_dir_all(c.join(".Spotlight-V100/Store-V2")).unwrap();
    fs::write(c.join(".Spotlight-V100/Store-V2/index"), b"x").unwrap();
    fs::write(c.join("._A001.R3D"), b"appledouble").unwrap();
    fs::write(c.join(".DS_Store"), b"ds").unwrap();
    let date = SystemTime::UNIX_EPOCH + Duration::from_secs(DATE_CARTE);
    for p in [
        rdc.join("A001_C001_0921XY_001.R3D"),
        rdc.join("A001_C001_0921XY_002.R3D"),
        rdc.join("A001_C001_0921XY.mov"),
        c.join("Scène 1 — prise 2.wav"),
    ] {
        fs::File::options()
            .write(true)
            .open(&p)
            .unwrap()
            .set_modified(date)
            .unwrap();
    }
    c
}

fn options(source: &Path, dests: &[PathBuf]) -> Options {
    let mut o = Options::new(source, dests.to_vec());
    // Les essais tournent sur un seul disque : la carte et les copies y
    // partagent le même volume, ce qui est refusé en usage réel.
    o.meme_volume_que_la_source = true;
    o.nom = "A001".into();
    o.operateur = "Test".into();
    o
}

fn lancer(o: &Options) -> (Rapport, Vec<Evenement>) {
    let mut ev = Vec::new();
    let r = decharger(o, &mut |e| ev.push(e)).expect("le déchargement doit démarrer");
    (r, ev)
}

fn chemins(r: &Rapport) -> Vec<&str> {
    r.fichiers.iter().map(|f| f.chemin.as_str()).collect()
}

/// Compare deux instantanés et nomme la première différence.
fn inchangee(dossier: &Path, avant: &[(String, Vec<u8>, SystemTime)]) {
    let apres = instantane(dossier);
    let noms =
        |v: &[(String, Vec<u8>, SystemTime)]| v.iter().map(|x| x.0.clone()).collect::<Vec<_>>();
    assert_eq!(noms(&apres), noms(avant), "la carte a changé (entrées)");
    for (a, b) in avant.iter().zip(&apres) {
        assert!(a.1 == b.1, "la carte a changé : contenu de {}", a.0);
        assert_eq!(a.2, b.2, "la carte a changé : date de {}", a.0);
    }
}

fn instantane(dossier: &Path) -> Vec<(String, Vec<u8>, SystemTime)> {
    let mut v: Vec<_> = walkdir::WalkDir::new(dossier)
        .sort_by_file_name()
        .into_iter()
        .filter_map(|e| e.ok())
        .map(|e| {
            // Métadonnées lues sur le fichier lui-même : sous Windows, celles
            // de l'énumération du dossier sont une copie qui peut être en retard.
            let m = fs::symlink_metadata(e.path()).unwrap();
            let contenu = if m.is_file() {
                fs::read(e.path()).unwrap()
            } else {
                Vec::new()
            };
            (
                e.path()
                    .strip_prefix(dossier)
                    .unwrap()
                    .display()
                    .to_string(),
                contenu,
                m.modified().unwrap(),
            )
        })
        .collect();
    v.sort_by(|a, b| a.0.cmp(&b.0));
    v
}

fn copie<'a>(r: &'a Rapport, fin: &str, d: usize) -> &'a offload_core::rapport::Copie {
    r.fichiers
        .iter()
        .find(|f| f.chemin.ends_with(fin))
        .and_then(|f| f.copie(d))
        .expect("copie attendue")
}

// -------------------------------------------------------------------------

#[test]
fn copie_conforme_complete_vers_deux_destinations() {
    let t = base();
    let source = carte(t.path());
    let avant = instantane(&source);
    let dests = vec![t.path().join("d1/A001"), t.path().join("d2/A001")];
    let (r, ev) = lancer(&options(&source, &dests));

    // La carte n'a pas été touchée.
    inchangee(&source, &avant);

    assert_eq!(r.nb_fichiers, 5, "{:?}", chemins(&r));
    assert!(r.exclus.iter().any(|e| e == ".Spotlight-V100"));
    assert!(r.exclus.iter().any(|e| e == "._A001.R3D"));
    for f in &r.fichiers {
        assert_eq!(f.copies.len(), 2);
        for c in &f.copies {
            assert!(c.niveau >= Niveau::CachePossible, "{} : {:?}", f.chemin, c);
            #[cfg(target_os = "linux")]
            assert_eq!(c.niveau, Niveau::Support, "{} : {:?}", f.chemin, c);
        }
        assert_eq!(f.empreintes.len(), 2, "xxh128 + xxh64");
    }

    for d in &dests {
        // Contenu identique, octet pour octet.
        for f in &r.fichiers {
            let a = fs::read(source.join(&f.chemin)).unwrap();
            let b = fs::read(d.join(&f.chemin)).unwrap();
            assert_eq!(a, b, "{}", f.chemin);
        }
        // Dossier vide recréé, exclus non copiés, aucun résidu.
        assert!(d.join("A001_0921XY.RDM/VIDE").is_dir());
        assert!(!d.join(".Spotlight-V100").exists());
        assert!(!d.join(".DS_Store").exists());
        assert!(!d.join("._A001.R3D").exists());
        assert!(residus(d).is_empty());
        // Dates de modification conservées.
        let date = SystemTime::UNIX_EPOCH + Duration::from_secs(DATE_CARTE);
        let m =
            fs::metadata(d.join("A001_0921XY.RDM/A001_C001_0921XY.RDC/A001_C001_0921XY_001.R3D"))
                .unwrap()
                .modified()
                .unwrap();
        assert_eq!(m, date);
        // Manifestes et rapports.
        assert!(d.join("ascmhl/ascmhl_chain.xml").is_file());
        assert!(d.join("_offload/rapport.html").is_file());
        assert!(d.join("_offload/journal.log").is_file());
        let json = fs::read_to_string(d.join("_offload/rapport.json")).unwrap();
        let relu: Rapport = serde_json::from_str(&json).unwrap();
        assert_eq!(relu.nb_fichiers, 5);
        let h = ascmhl::lire_historique(d).unwrap();
        assert_eq!(h[0].entrees.len(), 5);
        assert!(
            offload_core::mhl1::trouver(d).len() == 1,
            "MHL v1.1 attendu"
        );
    }

    // Deux dossiers sur le même disque : jamais « bon à formater ».
    assert_ne!(r.verdict, Verdict::BonAFormater);
    assert!(r.remarques.iter().any(|x| matches!(
        x,
        Remarque::MemeDisquePhysique { .. }
            | Remarque::CachePossible { .. }
            | Remarque::SurLeDisqueDeLaCarte { .. }
            | Remarque::DisquesNonIdentifies { .. }
    )));

    assert!(ev.iter().any(|e| matches!(e, Evenement::Copie { .. })));
    assert!(ev.iter().any(|e| matches!(e, Evenement::Avancement(_))));
}

#[test]
fn bit_retourne_a_lecriture_detecte_sur_cette_destination_seulement() {
    let t = base();
    let source = carte(t.path());
    let dests = vec![t.path().join("d1/A001"), t.path().join("d2/A001")];
    let mut o = options(&source, &dests);
    o.fautes = Plan {
        pannes: vec![Panne::BitEcrit {
            destination: 1,
            fichier: "_001.R3D".into(),
            octet: 12 * 1024 * 1024 + 77,
        }],
        ..Default::default()
    };
    let (r, _) = lancer(&o);
    let c = copie(&r, "_001.R3D", 1);
    assert_eq!(c.niveau, Niveau::Echec);
    assert_eq!(c.erreur.as_ref().unwrap().code, Code::EmpreinteDifferente);
    assert!(copie(&r, "_001.R3D", 0).niveau > Niveau::Echec);
    // La copie fautive n'existe sous aucun nom ; le reste est là.
    let p = "A001_0921XY.RDM/A001_C001_0921XY.RDC/A001_C001_0921XY_001.R3D";
    assert!(!dests[1].join(p).exists());
    assert!(residus(&dests[1]).is_empty());
    assert!(dests[1]
        .join("A001_0921XY.RDM/A001_C001_0921XY.RDC/A001_C001_0921XY_002.R3D")
        .is_file());
    assert_eq!(r.verdict, Verdict::Echec);
    // Le manifeste de la destination fautive ne mentionne pas le fichier.
    let h = ascmhl::lire_historique(&dests[1]).unwrap();
    assert!(!h[0].entrees.iter().any(|e| e.chemin == p));
}

#[test]
fn disque_plein_en_pleine_copie() {
    let t = base();
    let source = carte(t.path());
    let dests = vec![t.path().join("d1/A001"), t.path().join("d2/A001")];
    let mut o = options(&source, &dests);
    o.fautes.pannes = vec![Panne::DisquePlein {
        destination: 0,
        apres: 9 * 1024 * 1024,
    }];
    let (r, _) = lancer(&o);
    let d0 = &r.destinations[0];
    assert_eq!(d0.erreur_fatale.as_ref().unwrap().code, Code::DisquePlein);
    // Ordre de copie : le .mov (700 ko) passe avant le _001.R3D (20 Mio),
    // pendant lequel le disque se remplit ; tout ce qui suit échoue.
    assert!(copie(&r, ".mov", 0).niveau > Niveau::Echec);
    for fin in ["_001.R3D", "_002.R3D", "vide.txt", ".wav"] {
        assert_eq!(copie(&r, fin, 0).niveau, Niveau::Echec, "{fin}");
    }
    assert!(r
        .fichiers
        .iter()
        .all(|f| f.copie(1).unwrap().niveau > Niveau::Echec));
    assert!(residus(&dests[0]).is_empty());
    assert_eq!(r.verdict, Verdict::Echec);
}

#[test]
fn disque_debranche() {
    let t = base();
    let source = carte(t.path());
    let dests = vec![t.path().join("d1/A001"), t.path().join("d2/A001")];
    let mut o = options(&source, &dests);
    o.fautes.pannes = vec![Panne::Debranchement {
        destination: 1,
        fichier: "_002.R3D".into(),
    }];
    let (r, _) = lancer(&o);
    assert_eq!(
        r.destinations[1].erreur_fatale.as_ref().unwrap().code,
        Code::DestinationPerdue
    );
    assert!(
        copie(&r, "_001.R3D", 1).niveau > Niveau::Echec,
        "fichier d'avant conservé"
    );
    assert_eq!(copie(&r, "_002.R3D", 1).niveau, Niveau::Echec);
    assert_eq!(copie(&r, ".wav", 1).niveau, Niveau::Echec);
    assert!(r
        .fichiers
        .iter()
        .all(|f| f.copie(0).unwrap().niveau > Niveau::Echec));
}

#[test]
fn ecriture_perdue_sans_erreur_signalee() {
    let t = base();
    let source = carte(t.path());
    let dests = vec![t.path().join("d1/A001")];
    let mut o = options(&source, &dests);
    o.fautes.pannes = vec![Panne::EcritureCourte {
        destination: 0,
        fichier: ".mov".into(),
    }];
    let (r, _) = lancer(&o);
    let c = copie(&r, ".mov", 0);
    assert_eq!(c.erreur.as_ref().unwrap().code, Code::TailleDifferente);
}

#[test]
fn secteur_illisible_sur_la_carte() {
    let t = base();
    let source = carte(t.path());
    let dests = vec![t.path().join("d1/A001"), t.path().join("d2/A001")];
    let mut o = options(&source, &dests);
    o.fautes.pannes = vec![Panne::LectureSource {
        fichier: "_001.R3D".into(),
        apres: 10 * 1024 * 1024,
    }];
    let (r, ev) = lancer(&o);
    let f = r
        .fichiers
        .iter()
        .find(|f| f.chemin.ends_with("_001.R3D"))
        .unwrap();
    assert_eq!(f.erreur_source.as_ref().unwrap().code, Code::LectureSource);
    for (d, dossier) in dests.iter().enumerate() {
        assert_eq!(f.copie(d).unwrap().niveau, Niveau::Echec);
        assert!(residus(dossier).is_empty());
    }
    // Les fichiers suivants sont copiés normalement.
    assert!(copie(&r, "_002.R3D", 0).niveau > Niveau::Echec);
    assert_eq!(r.verdict, Verdict::Echec);
    assert!(ev.iter().any(|e| matches!(e, Evenement::Source { .. })));
}

#[test]
fn carte_modifiee_pendant_la_lecture() {
    let t = base();
    let source = carte(t.path());
    let dests = vec![t.path().join("d1/A001")];
    let mut o = options(&source, &dests);
    let cible = source.join("A001_0921XY.RDM/A001_C001_0921XY.RDC/A001_C001_0921XY_001.R3D");
    o.fautes.apres_bloc_source = Some(Arc::new(move |rel: &str, lus: u64| {
        if rel.ends_with("_001.R3D") && lus == 8 * 1024 * 1024 {
            use std::io::Write;
            let mut f = fs::OpenOptions::new().append(true).open(&cible).unwrap();
            f.write_all(b"la camera ecrit encore").unwrap();
        }
    }));
    let (r, _) = lancer(&o);
    let f = r
        .fichiers
        .iter()
        .find(|f| f.chemin.ends_with("_001.R3D"))
        .unwrap();
    assert_eq!(f.erreur_source.as_ref().unwrap().code, Code::SourceModifiee);
    assert_eq!(f.copie(0).unwrap().niveau, Niveau::Echec);
    assert_eq!(r.verdict, Verdict::Echec);
}

#[test]
fn relecture_de_la_carte_instable() {
    let t = base();
    let source = carte(t.path());
    let dests = vec![t.path().join("d1/A001")];
    let mut o = options(&source, &dests);
    o.mode = ModeVerif::CompletCarte;
    o.fautes.pannes = vec![Panne::SourceInstable {
        fichier: ".mov".into(),
    }];
    let (r, _) = lancer(&o);
    assert!(r
        .fichiers
        .iter()
        .filter(|f| !f.chemin.ends_with(".mov"))
        .all(|f| f.relecture_carte == Some(true)));
    assert!(r
        .remarques
        .iter()
        .any(|x| matches!(x, Remarque::RelectureCarteDifferente { fichiers: 1 })));
    assert_eq!(r.verdict, Verdict::Echec);
}

#[test]
fn annulation_propre() {
    let t = base();
    let source = carte(t.path());
    let dests = vec![t.path().join("d1/A001")];
    let mut o = options(&source, &dests);
    let jeton = o.annulation.clone();
    o.fautes.apres_bloc_source = Some(Arc::new(move |rel: &str, lus: u64| {
        if rel.ends_with("_001.R3D") && lus >= 8 * 1024 * 1024 {
            jeton.annuler();
        }
    }));
    let (r, _) = lancer(&o);
    assert!(r.annule);
    assert_eq!(r.verdict, Verdict::Annule);
    assert!(
        residus(&dests[0]).is_empty(),
        "aucun fichier temporaire ne doit rester"
    );
    assert!(r.fichiers.iter().all(|f| f.copies.len() == 1));
}

#[test]
fn mode_taille_seule_jamais_bon_a_formater() {
    let t = base();
    let source = carte(t.path());
    let dests = vec![t.path().join("d1/A001"), t.path().join("d2/A001")];
    let mut o = options(&source, &dests);
    o.mode = ModeVerif::Taille;
    let (r, _) = lancer(&o);
    assert!(r
        .fichiers
        .iter()
        .all(|f| f.copies.iter().all(|c| c.niveau == Niveau::Taille)));
    assert_eq!(r.verdict, Verdict::ConformeNePasFormater);
    assert!(r.remarques.contains(&Remarque::ModeTaille));
}

#[test]
fn destination_existante_refusee_sans_rien_ecrire() {
    let t = base();
    let source = carte(t.path());
    let d = t.path().join("d1/A001");
    fs::create_dir_all(&d).unwrap();
    fs::write(d.join("precieux.R3D"), b"ne pas toucher").unwrap();
    let refus = decharger(&options(&source, std::slice::from_ref(&d)), &mut |_| {}).unwrap_err();
    assert!(refus
        .erreurs
        .iter()
        .any(|e| e.code == Code::DestinationExiste));
    assert_eq!(fs::read(d.join("precieux.R3D")).unwrap(), b"ne pas toucher");
    assert_eq!(fs::read_dir(&d).unwrap().count(), 1);
}

#[test]
fn tous_les_algorithmes_et_md5_seul() {
    let t = base();
    let source = carte(t.path());
    let d = vec![t.path().join("d1/A001")];
    let mut o = options(&source, &d);
    o.algos = Algo::TOUS.to_vec();
    let (r, _) = lancer(&o);
    assert!(r.fichiers.iter().all(|f| f.empreintes.len() == 6));
    assert_eq!(r.algo_comparaison, Algo::Xxh128);
    let h = ascmhl::lire_historique(&d[0]).unwrap();
    assert_eq!(h[0].entrees[0].empreintes.len(), 6);

    let d = vec![t.path().join("d2/A001")];
    let mut o = options(&source, &d);
    o.algos = vec![Algo::Md5];
    let (r, _) = lancer(&o);
    assert_eq!(r.algo_comparaison, Algo::Md5);
    assert!(r
        .fichiers
        .iter()
        .all(|f| f.copie(0).unwrap().niveau > Niveau::Echec));
}

#[test]
fn verification_dun_dossier_apres_coup() {
    let t = base();
    let source = carte(t.path());
    let d = t.path().join("d1/A001");
    lancer(&options(&source, std::slice::from_ref(&d)));
    let jeton = Default::default();

    let b = verification::verifier(&d, &jeton, &mut |_| {}).unwrap();
    assert!(b.conforme(), "{:?}", b);
    assert_eq!(b.format, "ascmhl");
    assert_eq!(b.nb_fichiers, 5);

    // Corruption, disparition, ajout.
    let r3d = d.join("A001_0921XY.RDM/A001_C001_0921XY.RDC/A001_C001_0921XY_002.R3D");
    let mut octets = fs::read(&r3d).unwrap();
    octets[1000] ^= 0x10;
    fs::write(&r3d, octets).unwrap();
    fs::remove_file(d.join("Scène 1 — prise 2.wav")).unwrap();
    fs::write(d.join("intrus.txt"), b"?").unwrap();
    let b = verification::verifier(&d, &jeton, &mut |_| {}).unwrap();
    assert!(!b.conforme());
    let codes: Vec<Code> = b.problemes.iter().map(|p| p.erreur.code).collect();
    assert!(codes.contains(&Code::EmpreinteDifferente), "{codes:?}");
    assert!(codes.contains(&Code::Manquant), "{codes:?}");
    assert_eq!(b.inattendus, vec!["intrus.txt"]);

    // Sans historique ASC MHL : repli sur le MHL v1.1.
    fs::remove_dir_all(d.join("ascmhl")).unwrap();
    fs::remove_file(d.join("intrus.txt")).unwrap();
    let b = verification::verifier(&d, &jeton, &mut |_| {}).unwrap();
    assert_eq!(b.format, "mhl-v1.1");
    assert!(b
        .problemes
        .iter()
        .any(|p| p.erreur.code == Code::EmpreinteDifferente));
}

#[test]
fn bon_a_formater_exige_deux_disques_physiques() {
    let t = base();
    let source = carte(t.path());
    let dests = vec![t.path().join("d1/A001"), t.path().join("d2/A001")];
    let (mut r, _) = lancer(&options(&source, &dests));
    // Simule deux disques physiques distincts (et une carte sur un
    // troisième) et des relectures prouvées.
    r.volume_source.as_mut().unwrap().id_disque = Some("carte".into());
    for (i, d) in r.destinations.iter_mut().enumerate() {
        let v = d.volume.as_mut().unwrap();
        v.id_disque = Some(format!("disque{i}"));
        v.reseau = false;
    }
    for f in r.fichiers.iter_mut() {
        for c in f.copies.iter_mut() {
            c.niveau = Niveau::Support;
        }
    }
    r.conclure(vec![]);
    assert_eq!(r.verdict, Verdict::BonAFormater, "{:?}", r.remarques);

    // Même disque : refusé.
    for d in r.destinations.iter_mut() {
        d.volume.as_mut().unwrap().id_disque = Some("disque0".into());
    }
    r.conclure(vec![]);
    assert_eq!(r.verdict, Verdict::ConformeNePasFormater);

    // Une copie dont la relecture n'est pas prouvée : refusé.
    for (i, d) in r.destinations.iter_mut().enumerate() {
        d.volume.as_mut().unwrap().id_disque = Some(format!("disque{i}"));
    }
    r.fichiers[0].copies[1].niveau = Niveau::CachePossible;
    r.conclure(vec![]);
    assert_eq!(r.verdict, Verdict::ConformeNePasFormater);
    r.fichiers[0].copies[1].niveau = Niveau::Support;
    r.conclure(vec![]);
    assert_eq!(r.verdict, Verdict::BonAFormater);

    // Disque physique non identifié : la copie ne compte pas (revue C1).
    r.destinations[1].volume.as_mut().unwrap().id_disque = None;
    r.conclure(vec![]);
    assert_eq!(r.verdict, Verdict::ConformeNePasFormater);
    assert!(r
        .remarques
        .iter()
        .any(|x| matches!(x, Remarque::DisquesNonIdentifies { .. })));
    r.destinations[1].volume = None;
    r.conclure(vec![]);
    assert_eq!(r.verdict, Verdict::ConformeNePasFormater);

    // Copie sur le disque de la carte : ne compte pas (revue H1).
    r.destinations[1].volume = r.destinations[0].volume.clone();
    r.destinations[1].volume.as_mut().unwrap().id_disque = Some("carte".into());
    r.conclure(vec![]);
    assert_eq!(r.verdict, Verdict::ConformeNePasFormater);
    assert!(r
        .remarques
        .iter()
        .any(|x| matches!(x, Remarque::SurLeDisqueDeLaCarte { destination: 1 })));

    // Volume LVM / RAID qui partage un disque avec une autre copie.
    r.destinations[0].volume.as_mut().unwrap().id_disque = Some("sda,sdb".into());
    r.destinations[1].volume.as_mut().unwrap().id_disque = Some("sdb".into());
    r.conclure(vec![]);
    assert_eq!(r.verdict, Verdict::ConformeNePasFormater);
}

/// Revue C1/H1 en conditions réelles : sans l'autorisation des essais, une
/// destination sur le volume de la carte est refusée, sans rien y écrire.
#[test]
fn destination_sur_le_volume_de_la_carte_refusee() {
    let t = base();
    let source = carte(t.path());
    let avant = instantane(&source);
    let mut o = options(&source, &[source.join("../SAUVEGARDE/X")]);
    o.meme_volume_que_la_source = false;
    let refus = match decharger(&o, &mut |_| {}) {
        Err(r) => r,
        Ok(r) => panic!(
            "copie acceptée sur le volume de la carte : source {:?}, destination {:?}",
            r.volume_source, r.destinations[0].volume
        ),
    };
    assert!(
        refus
            .erreurs
            .iter()
            .any(|e| e.code == Code::DestinationSurLaCarte),
        "{:?}",
        refus.erreurs
    );
    assert!(!refus
        .erreurs
        .iter()
        .any(|e| e.code == Code::CreationImpossible));
    inchangee(&source, &avant);
    assert!(!t.path().join("SAUVEGARDE").exists());
}

#[test]
fn carte_sans_fichier_jamais_bon_a_formater() {
    let t = base();
    let s = t.path().join("VIDE");
    fs::create_dir_all(s.join("DCIM/100")).unwrap();
    let dests = vec![t.path().join("d1/X"), t.path().join("d2/X")];
    let (mut r, _) = lancer(&options(&s, &dests));
    r.volume_source.as_mut().unwrap().id_disque = Some("carte".into());
    for (i, d) in r.destinations.iter_mut().enumerate() {
        d.volume.as_mut().unwrap().id_disque = Some(format!("k{i}"));
    }
    r.conclure(vec![]);
    assert_ne!(r.verdict, Verdict::BonAFormater);
    assert!(r.remarques.contains(&Remarque::CarteVide));
    assert!(dests[0].join("DCIM/100").is_dir());
}

/// Revue M2 : les rapports d'un déchargement précédent présents sur la
/// source ne sont ni copiés ni écrasés.
#[test]
fn rapports_precedents_de_la_source_non_copies() {
    let t = base();
    let s = carte(t.path());
    fs::create_dir_all(s.join("_offload")).unwrap();
    fs::write(s.join("_offload/rapport.json"), b"ANCIEN").unwrap();
    let d = vec![t.path().join("d1/X")];
    let (r, _) = lancer(&options(&s, &d));
    assert!(!r.fichiers.iter().any(|f| f.chemin.starts_with("_offload")));
    assert!(r.exclus.iter().any(|e| e == "_offload"));
    let nouveau = fs::read_to_string(d[0].join("_offload/rapport.json")).unwrap();
    assert!(nouveau.contains("\"verdict\""));
}

/// Revue M2 : un échec d'écriture du rapport rend la destination fautive
/// et le verdict est recalculé.
#[test]
fn echec_decriture_du_rapport_pris_en_compte() {
    let t = base();
    let s = carte(t.path());
    let d = vec![t.path().join("d1/X"), t.path().join("d2/X")];
    let mut o = options(&s, &d);
    // Un fichier « _offload » apparaît sur la destination 1 pendant la copie.
    let gene = d[1].join("_offload");
    o.fautes.apres_bloc_source = Some(Arc::new(move |rel: &str, _| {
        if rel.ends_with(".wav") && !gene.exists() {
            let _ = fs::write(&gene, b"bouchon");
        }
    }));
    let (r, _) = lancer(&o);
    assert_eq!(
        r.destinations[1].erreur_fatale.as_ref().unwrap().code,
        Code::EcritureImpossible
    );
    assert!(r
        .remarques
        .iter()
        .any(|x| matches!(x, Remarque::DestinationEnEchec { destination: 1 })));
    let relu: Rapport =
        serde_json::from_str(&fs::read_to_string(d[0].join("_offload/rapport.json")).unwrap())
            .unwrap();
    assert_eq!(
        relu.verdict, r.verdict,
        "le rapport écrit porte le verdict recalculé"
    );
}

#[test]
fn carte_changee_depuis_la_mise_en_file() {
    let t = base();
    let s = carte(t.path());
    let d = vec![t.path().join("d1/X")];
    let mut o = options(&s, &d);
    o.empreinte_attendue = Some("pas la bonne".into());
    let refus = decharger(&o, &mut |_| {}).unwrap_err();
    assert!(refus
        .erreurs
        .iter()
        .any(|e| e.code == Code::CarteDifferente));
    assert!(!d[0].exists());
    let inv = offload_core::inventaire::inventorier(&s).unwrap();
    o.empreinte_attendue = Some(offload_core::registre::empreinte_inventaire(&inv));
    assert!(decharger(&o, &mut |_| {}).is_ok());
}

/// Revue : 25 annulations à des instants différents, aucun résidu, aucun
/// fichier final qui ne soit identique à la carte.
#[test]
fn annulations_aleatoires_sans_residu() {
    let t = base();
    let c = t.path().join("CARTE");
    fs::create_dir_all(&c).unwrap();
    for i in 0..40usize {
        let n = (i * 7919 % 13) * 300_000 + i;
        fs::write(c.join(format!("f{i:03}.bin")), vec![(i % 251) as u8; n]).unwrap();
    }
    for essai in 0..25u64 {
        let d = vec![
            t.path().join(format!("a{essai}/X")),
            t.path().join(format!("b{essai}/X")),
        ];
        let mut o = options(&c, &d);
        let jeton = o.annulation.clone();
        let seuil = (essai * 2_654_435_761) % 40;
        let compte = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let k = compte.clone();
        o.fautes.apres_bloc_source = Some(Arc::new(move |_: &str, _: u64| {
            if k.fetch_add(1, std::sync::atomic::Ordering::SeqCst) >= seuil {
                jeton.annuler();
            }
        }));
        let (r, _) = lancer(&o);
        for (i, dd) in d.iter().enumerate() {
            assert!(residus(dd).is_empty());
            for f in &r.fichiers {
                let p = dd.join(&f.chemin);
                let niveau = f.copie(i).unwrap().niveau;
                if p.exists() {
                    assert!(niveau > Niveau::Echec);
                    assert_eq!(fs::read(&p).unwrap(), fs::read(c.join(&f.chemin)).unwrap());
                } else {
                    assert_eq!(niveau, Niveau::Echec);
                }
            }
        }
        assert_ne!(r.verdict, Verdict::BonAFormater);
    }
}

/// Interopérabilité : l'outil de référence de l'ASC doit valider nos
/// manifestes. Le test est ignoré (avec un message) si l'outil est absent ;
/// la CI l'installe et définit ASCMHL_OBLIGATOIRE pour qu'il ne puisse pas
/// être ignoré là-bas.
#[test]
fn manifestes_valides_par_loutil_de_reference_asc() {
    let outil = std::env::var("ASCMHL_DEBUG").unwrap_or_else(|_| "ascmhl-debug".into());
    let present = Command::new(&outil).arg("--help").output().is_ok();
    if !present {
        assert!(
            std::env::var("ASCMHL_OBLIGATOIRE").is_err(),
            "ascmhl-debug introuvable alors que la CI l'exige"
        );
        eprintln!("ascmhl-debug absent : test d'interopérabilité non exécuté");
        return;
    }
    let t = base();
    let source = carte(t.path());
    let d = t.path().join("d1/A001");
    let mut o = options(&source, std::slice::from_ref(&d));
    o.algos = vec![
        Algo::Xxh128,
        Algo::Xxh64,
        Algo::Md5,
        Algo::Sha1,
        Algo::C4,
        Algo::Xxh3,
    ];
    let (r, _) = lancer(&o);
    assert!(r
        .fichiers
        .iter()
        .all(|f| f.copie(0).unwrap().niveau > Niveau::Echec));

    let sortie = Command::new(&outil).arg("verify").arg(&d).output().unwrap();
    assert!(
        sortie.status.success(),
        "ascmhl-debug verify a refusé nos manifestes :\n{}\n{}",
        String::from_utf8_lossy(&sortie.stdout),
        String::from_utf8_lossy(&sortie.stderr)
    );
    let diff = std::env::var("ASCMHL").unwrap_or_else(|_| "ascmhl".into());
    let sortie = Command::new(&diff).arg("diff").arg(&d).output().unwrap();
    let texte = String::from_utf8_lossy(&sortie.stdout).to_string()
        + &String::from_utf8_lossy(&sortie.stderr);
    assert!(
        sortie.status.success() && !texte.contains("ERROR"),
        "ascmhl diff :\n{texte}"
    );

    // Et il doit détecter une corruption faite après coup.
    let wav = d.join("Scène 1 — prise 2.wav");
    let mut octets = fs::read(&wav).unwrap();
    octets[3] ^= 1;
    fs::write(&wav, octets).unwrap();
    let sortie = Command::new(&outil).arg("verify").arg(&d).output().unwrap();
    assert!(!sortie.status.success(), "la corruption doit être détectée");
}
