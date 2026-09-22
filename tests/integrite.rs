//! Tests d'intégrité : on corrompt volontairement des copies pour vérifier
//! que le moteur les refuse. Un outil qui certifie des rushs ne vaut que par
//! ce qu'il sait détecter.

use offload_core::copy::{self, Options};
use offload_core::hash::Algo;
use offload_core::report::Etat;
use offload_core::{mhl, scan, verify};
use std::fs;
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

/// Contenu pseudo-aléatoire déterministe : ni compressible, ni répétitif.
fn contenu(graine: u64, taille: usize) -> Vec<u8> {
    let mut v = Vec::with_capacity(taille);
    let mut x = graine.wrapping_mul(6364136223846793005).wrapping_add(1);
    while v.len() < taille {
        x = x
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        v.extend_from_slice(&x.to_le_bytes());
    }
    v.truncate(taille);
    v
}

fn ecrire(racine: &Path, relatif: &str, donnees: &[u8]) {
    let chemin = racine.join(relatif);
    fs::create_dir_all(chemin.parent().unwrap()).unwrap();
    fs::write(&chemin, donnees).unwrap();
}

/// Carte caméra factice : structure RED (clip découpé en segments), un MXF
/// de taille non alignée, des métadonnées et des fichiers système macOS.
fn carte_factice(racine: &Path) {
    fs::create_dir_all(racine).unwrap();
    ecrire(
        racine,
        "A001_0921XY.RDC/A001_C003_0921XY_001.R3D",
        &contenu(1, 3 * 1024 * 1024),
    );
    ecrire(
        racine,
        "A001_0921XY.RDC/A001_C003_0921XY_002.R3D",
        &contenu(2, 2 * 1024 * 1024 + 7),
    );
    ecrire(
        racine,
        "A001_0921XY.RDC/A001_C003_0921XY.RMD",
        &contenu(3, 4096),
    );
    ecrire(
        racine,
        "Clip/C0001.MXF",
        &contenu(4, 5 * 1024 * 1024 + 1234),
    );
    ecrire(racine, "Clip/C0002.MXF", &contenu(5, 1023));
    ecrire(racine, "AVCHD/lecture.txt", b"note de production");
    // Fichiers système : ne doivent jamais être copies.
    ecrire(racine, ".DS_Store", &contenu(6, 6148));
    ecrire(racine, "Clip/._C0001.MXF", &contenu(7, 82));
}

struct Contexte {
    _racine: tempfile::TempDir,
    source: PathBuf,
    dest_a: PathBuf,
    dest_b: PathBuf,
}

fn preparer() -> Contexte {
    // Les tests s'exécutent sous le dossier de travail pour rester sur un
    // système de fichiers qui accepte la lecture directe.
    let racine = tempfile::Builder::new()
        .prefix("offload-test-")
        .tempdir_in(env!("CARGO_MANIFEST_DIR"))
        .unwrap();
    let source = racine.path().join("carte");
    let dest_a = racine.path().join("disque_a");
    let dest_b = racine.path().join("disque_b");
    carte_factice(&source);
    fs::create_dir_all(&dest_a).unwrap();
    fs::create_dir_all(&dest_b).unwrap();
    Contexte {
        _racine: racine,
        source,
        dest_a,
        dest_b,
    }
}

fn options(ctx: &Contexte, algos: Vec<Algo>) -> Options {
    Options {
        source: ctx.source.clone(),
        destinations: vec![ctx.dest_a.join("A001"), ctx.dest_b.join("A001")],
        algos,
        verifier_source: false,
        nom: "A001".to_string(),
    }
}

fn decharger(opts: &Options) -> offload_core::report::Rapport {
    copy::decharger(opts, &mut |_| {}).expect("le déchargement doit aboutir")
}

/// Dépose le MHL comme le fait la ligne de commande, pour pouvoir
/// revérifier le dossier ensuite.
fn poser_mhl(rapport: &offload_core::report::Rapport, dossier: &Path, index: usize) -> PathBuf {
    let chemin = dossier.join("A001.mhl");
    mhl::ecrire(rapport, index, &chemin).unwrap();
    chemin
}

// ---------------------------------------------------------------------------

#[test]
fn copie_conforme_sur_deux_destinations() {
    let ctx = preparer();
    let opts = options(&ctx, vec![Algo::Xxh64]);
    let rapport = decharger(&opts);

    assert_eq!(
        rapport.etat_global,
        Etat::Ok,
        "{:?}",
        rapport.avertissements
    );
    assert_eq!(rapport.nb_fichiers, 6, "6 fichiers utiles attendus");
    // Les deux segments et le .RMD appartiennent au même clip RED.
    assert_eq!(rapport.nb_clips, 4);

    for d in &rapport.destinations {
        assert_eq!(d.etat, Etat::Ok);
        assert_eq!(d.fichiers_echec, 0);
        assert_eq!(d.fichiers_ok, 6);
    }

    // Contenu identique, octet pour octet.
    for f in &rapport.fichiers {
        let origine = fs::read(ctx.source.join(&f.chemin)).unwrap();
        for dest in [&ctx.dest_a, &ctx.dest_b] {
            let copie = fs::read(dest.join("A001").join(&f.chemin)).unwrap();
            assert_eq!(origine, copie, "contenu différent pour {}", f.chemin);
        }
    }
}

#[test]
fn fichiers_systeme_non_copies() {
    let ctx = preparer();
    let rapport = decharger(&options(&ctx, vec![Algo::Xxh64]));

    assert!(!ctx.dest_a.join("A001/.DS_Store").exists());
    assert!(!ctx.dest_a.join("A001/Clip/._C0001.MXF").exists());
    assert_eq!(rapport.ignores.len(), 2, "{:?}", rapport.ignores);
}

#[test]
fn aucun_fichier_temporaire_ne_subsiste() {
    let ctx = preparer();
    decharger(&options(&ctx, vec![Algo::Xxh64]));
    assert!(copy::residus(&ctx.dest_a).is_empty());
    assert!(copy::residus(&ctx.dest_b).is_empty());
}

#[test]
fn la_relecture_contourne_bien_le_cache() {
    let ctx = preparer();
    let rapport = decharger(&options(&ctx, vec![Algo::Xxh64]));

    for f in &rapport.fichiers {
        for v in &f.verifications {
            let mode = v.mode_lecture.expect("mode de lecture consigné");
            assert!(
                mode.fiable(),
                "la vérification de {} n'a pas atteint le support : {}",
                f.chemin,
                mode.libelle()
            );
        }
    }
}

#[test]
fn un_bit_modifie_est_detecte() {
    let ctx = preparer();
    let rapport = decharger(&options(&ctx, vec![Algo::Xxh64]));
    let dossier = ctx.dest_a.join("A001");
    poser_mhl(&rapport, &dossier, 0);

    // Un seul bit retourné au milieu d'un segment R3D.
    let cible = dossier.join("A001_0921XY.RDC/A001_C003_0921XY_001.R3D");
    let mut f = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&cible)
        .unwrap();
    f.seek(SeekFrom::Start(1_500_000)).unwrap();
    let mut octet = [0u8; 1];
    use std::io::Read;
    f.read_exact(&mut octet).unwrap();
    f.seek(SeekFrom::Start(1_500_000)).unwrap();
    f.write_all(&[octet[0] ^ 0b0000_0001]).unwrap();
    f.sync_all().unwrap();
    drop(f);

    let verif = verify::controler_dossier(&dossier).unwrap();
    assert_eq!(verif.etat, Etat::Echec);
    let ligne = verif
        .lignes
        .iter()
        .find(|l| l.fichier.ends_with("_001.R3D"))
        .unwrap();
    assert_eq!(ligne.etat, Etat::Echec);
    assert!(
        ligne.detail.as_ref().unwrap().contains("empreinte"),
        "{:?}",
        ligne.detail
    );
    // Les autres fichiers restent conformes : le diagnostic doit être précis.
    assert_eq!(verif.nb_echecs(), 1);
}

#[test]
fn un_fichier_tronque_est_detecte() {
    let ctx = preparer();
    let rapport = decharger(&options(&ctx, vec![Algo::Xxh64]));
    let dossier = ctx.dest_a.join("A001");
    poser_mhl(&rapport, &dossier, 0);

    let cible = dossier.join("Clip/C0001.MXF");
    let f = fs::OpenOptions::new().write(true).open(&cible).unwrap();
    f.set_len(1_000_000).unwrap();
    f.sync_all().unwrap();
    drop(f);

    let verif = verify::controler_dossier(&dossier).unwrap();
    assert_eq!(verif.etat, Etat::Echec);
    let ligne = verif
        .lignes
        .iter()
        .find(|l| l.fichier.ends_with("C0001.MXF"))
        .unwrap();
    assert!(
        ligne.detail.as_ref().unwrap().contains("taille"),
        "{:?}",
        ligne.detail
    );
}

#[test]
fn un_segment_de_clip_absent_est_detecte() {
    let ctx = preparer();
    let rapport = decharger(&options(&ctx, vec![Algo::Xxh64]));
    let dossier = ctx.dest_a.join("A001");
    poser_mhl(&rapport, &dossier, 0);

    // Un segment R3D manquant rend tout le plan inexploitable.
    fs::remove_file(dossier.join("A001_0921XY.RDC/A001_C003_0921XY_002.R3D")).unwrap();

    let verif = verify::controler_dossier(&dossier).unwrap();
    assert_eq!(verif.etat, Etat::Echec);
    assert_eq!(verif.absents.len(), 1);
    assert!(verif.absents[0].ends_with("_002.R3D"));
}

#[test]
fn un_fichier_en_trop_est_signale() {
    let ctx = preparer();
    let rapport = decharger(&options(&ctx, vec![Algo::Xxh64]));
    let dossier = ctx.dest_a.join("A001");
    poser_mhl(&rapport, &dossier, 0);

    fs::write(dossier.join("Clip/C0003.MXF"), b"fichier ajoute apres coup").unwrap();

    let verif = verify::controler_dossier(&dossier).unwrap();
    assert_eq!(verif.en_trop.len(), 1);
    assert!(verif.en_trop[0].ends_with("C0003.MXF"));
    // Un fichier en trop n'invalide pas les rushs vérifiés.
    assert_eq!(verif.nb_echecs(), 0);
}

#[test]
fn une_copie_interrompue_est_signalee() {
    let ctx = preparer();
    let rapport = decharger(&options(&ctx, vec![Algo::Xxh64]));
    let dossier = ctx.dest_a.join("A001");
    poser_mhl(&rapport, &dossier, 0);

    // Un fichier resté sous son nom temporaire : copie interrompue.
    fs::write(
        dossier.join("Clip/C0009.MXF.offload-part"),
        contenu(42, 4096),
    )
    .unwrap();

    let verif = verify::controler_dossier(&dossier).unwrap();
    assert_eq!(verif.residus.len(), 1);
    assert_eq!(
        verif.etat,
        Etat::Echec,
        "une copie interrompue doit être rouge"
    );
}

#[test]
fn le_mhl_relu_correspond_a_ce_qui_a_ete_ecrit() {
    let ctx = preparer();
    let rapport = decharger(&options(&ctx, vec![Algo::Xxh64, Algo::Md5]));
    let dossier = ctx.dest_a.join("A001");
    let chemin = poser_mhl(&rapport, &dossier, 0);

    let entrees = mhl::lire(&chemin).unwrap();
    assert_eq!(entrees.len(), rapport.nb_fichiers);

    for e in &entrees {
        let f = rapport
            .fichiers
            .iter()
            .find(|f| f.chemin == e.fichier)
            .unwrap_or_else(|| panic!("{} absent du rapport", e.fichier));
        assert_eq!(e.taille, f.taille);
        assert_eq!(e.empreintes.len(), 2, "xxHash64 et MD5 attendus");
        for (algo, valeur) in &e.empreintes {
            assert_eq!(valeur, f.empreinte(*algo).unwrap());
        }
    }

    let xml = fs::read_to_string(&chemin).unwrap();
    assert!(xml.contains(r#"<hashlist version="1.1">"#));
    assert!(xml.contains("<xxhash64be>"));
    assert!(xml.contains("<md5>"));
}

#[test]
fn un_dossier_de_destination_existant_est_refuse() {
    let ctx = preparer();
    let deja = ctx.dest_a.join("A001");
    fs::create_dir_all(deja.join("Clip")).unwrap();
    fs::write(
        deja.join("Clip/C0001.MXF"),
        b"rushs d'un tournage precedent",
    )
    .unwrap();

    let opts = options(&ctx, vec![Algo::Xxh64]);
    let erreur = copy::decharger(&opts, &mut |_| {}).unwrap_err();
    assert_eq!(erreur.kind(), std::io::ErrorKind::AlreadyExists);

    // Rien n'a été touché.
    assert_eq!(
        fs::read(deja.join("Clip/C0001.MXF")).unwrap(),
        b"rushs d'un tournage precedent"
    );
    assert!(!ctx.dest_b.join("A001").exists());
}

#[test]
fn espace_insuffisant_refuse_avant_toute_ecriture() {
    let ctx = preparer();
    let mut inventaire = scan::scanner(&ctx.source).unwrap();
    // Carte de 500 To : aucune destination ne peut l'accueillir.
    inventaire.octets_total = 500 * 1000 * 1000 * 1000 * 1000;

    let erreur =
        offload_core::preflight::controler(&inventaire, &[ctx.dest_a.join("A001")], &ctx.source)
            .unwrap_err();

    assert_eq!(erreur.kind(), std::io::ErrorKind::StorageFull);
    assert!(!ctx.dest_a.join("A001").exists());
}

#[test]
fn une_destination_dans_la_source_est_refusee() {
    let ctx = preparer();
    let erreur = copy::decharger(
        &Options {
            source: ctx.source.clone(),
            destinations: vec![ctx.source.join("copie")],
            algos: vec![Algo::Xxh64],
            verifier_source: false,
            nom: "copie".to_string(),
        },
        &mut |_| {},
    )
    .unwrap_err();
    assert_eq!(erreur.kind(), std::io::ErrorKind::InvalidInput);
}

#[test]
fn un_disque_qui_tombe_en_cours_de_copie_nempeche_pas_lautre() {
    let ctx = preparer();
    let opts = options(&ctx, vec![Algo::Xxh64]);
    let piege = ctx.dest_b.join("A001/Clip");

    // Au troisième fichier, la seconde destination devient inutilisable :
    // un fichier occupe la place du dossier « Clip » à venir. C'est le
    // scénario du disque débranché ou du support qui refuse d'écrire.
    let mut rapport = None;
    let r = copy::decharger(&opts, &mut |p| {
        if let offload_core::copy::Progression::Fichier { index: 3, .. } = p {
            fs::write(&piege, b"le disque ne repond plus").unwrap();
        }
    });
    rapport.replace(r.expect("la copie doit aboutir malgré la panne"));
    let rapport = rapport.unwrap();

    // Voyant rouge global, et la destination fautive est nommée.
    assert_eq!(rapport.etat_global, Etat::Echec);
    assert_eq!(
        rapport.destinations[0].etat,
        Etat::Ok,
        "le disque A doit rester bon"
    );
    assert_eq!(rapport.destinations[1].etat, Etat::Echec);
    assert!(rapport.destinations[1].erreur.is_some());

    // Le disque A a bien tous les fichiers, vérifiés et intacts.
    assert_eq!(rapport.destinations[0].fichiers_ok, rapport.nb_fichiers);
    for f in &rapport.fichiers {
        let origine = fs::read(ctx.source.join(&f.chemin)).unwrap();
        let copie = fs::read(ctx.dest_a.join("A001").join(&f.chemin)).unwrap();
        assert_eq!(origine, copie);
    }

    // Aucun fichier n'est déclaré vérifié partout alors qu'il manque sur B :
    // chaque fichier porte un échec explicite pour la destination tombée.
    let fichiers_apres_panne: Vec<_> = rapport
        .fichiers
        .iter()
        .filter(|f| f.chemin.starts_with("Clip/"))
        .collect();
    assert!(!fichiers_apres_panne.is_empty());
    for f in fichiers_apres_panne {
        assert!(
            !f.ok_partout(),
            "{} ne doit pas être donné pour conforme",
            f.chemin
        );
        let v = f.verifications.iter().find(|v| v.destination == 1).unwrap();
        assert_eq!(v.etat, Etat::Echec);
    }

    // Le MHL de la destination fautive n'atteste que des fichiers réellement
    // vérifiés : il ne doit jamais certifier ce qui n'a pas été écrit.
    let dossier_b = ctx.dest_b.join("A001");
    let mhl_b = dossier_b.join("A001.mhl");
    let attestes = mhl::ecrire(&rapport, 1, &mhl_b).unwrap();
    assert!(attestes < rapport.nb_fichiers);
    for e in mhl::lire(&mhl_b).unwrap() {
        assert!(
            dossier_b.join(&e.fichier).is_file(),
            "{} attesté au MHL mais absent du disque",
            e.fichier
        );
    }

    // Et aucune copie partielle ne porte un nom définitif.
    for r in copy::residus(&ctx.dest_b) {
        assert!(r.to_string_lossy().ends_with(".offload-part"));
    }
}

#[test]
fn la_relecture_de_la_source_est_consignee() {
    let ctx = preparer();
    let mut opts = options(&ctx, vec![Algo::Xxh64]);
    opts.verifier_source = true;
    let rapport = decharger(&opts);

    assert_eq!(rapport.source_reverifiee, Some(Etat::Ok));
    assert_eq!(rapport.etat_global, Etat::Ok);
}

#[test]
fn le_rapport_json_est_relisible() {
    let ctx = preparer();
    let rapport = decharger(&options(&ctx, vec![Algo::Xxh64]));
    let chemin = ctx.dest_a.join("rapport.json");
    rapport.ecrire_json(&chemin).unwrap();

    let relu: offload_core::report::Rapport =
        serde_json::from_str(&fs::read_to_string(&chemin).unwrap()).unwrap();
    assert_eq!(relu.nb_fichiers, rapport.nb_fichiers);
    assert_eq!(relu.etat_global, Etat::Ok);
    assert_eq!(relu.fichiers.len(), rapport.fichiers.len());
}
