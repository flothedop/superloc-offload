//! Interface en ligne de commande du moteur de déchargement.
//!
//! L'interface graphique viendra s'appuyer sur la bibliothèque ; cette
//! ligne de commande sert au développement, aux tests sur cartes réelles et
//! aux utilisateurs avancés.

use offload_core::copy::{self, Options, Progression};
use offload_core::hash::Algo;
use offload_core::preflight::octets_lisibles;
use offload_core::{mhl, verify};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

fn aide() -> &'static str {
    "\
SuperLoc Offload — copie et vérification de rushs

  offload copier --source <dossier> --dest <dossier> [--dest <dossier>...]
                 [--nom <nom>] [--empreinte xxh64|md5|xxh64,md5]
                 [--verifier-source]

  offload verifier --dossier <dossier>
        Revérifie un dossier déjà copié à partir de son fichier MHL.

Options :
  --nom              Nom du déchargement (carte, volume). Par défaut : nom du dossier source.
  --empreinte        Algorithmes, séparés par des virgules. Par défaut : xxh64.
  --verifier-source  Relit aussi la carte source après copie (détecte un lecteur défaillant).
"
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() || args[0] == "--aide" || args[0] == "-h" || args[0] == "--help" {
        print!("{}", aide());
        return ExitCode::SUCCESS;
    }

    let resultat = match args[0].as_str() {
        "copier" | "copy" => commande_copier(&args[1..]),
        "verifier" | "verify" => commande_verifier(&args[1..]),
        autre => Err(format!("commande inconnue : {autre}\n\n{}", aide())),
    };

    match resultat {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("\n✖ {e}");
            ExitCode::FAILURE
        }
    }
}

fn valeur(args: &[String], i: &mut usize, nom: &str) -> Result<String, String> {
    *i += 1;
    args.get(*i)
        .cloned()
        .ok_or_else(|| format!("valeur manquante après {nom}"))
}

fn commande_copier(args: &[String]) -> Result<bool, String> {
    let mut source: Option<PathBuf> = None;
    let mut destinations: Vec<PathBuf> = Vec::new();
    let mut nom: Option<String> = None;
    let mut algos: Vec<Algo> = vec![Algo::Xxh64];
    let mut verifier_source = false;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--source" => source = Some(PathBuf::from(valeur(args, &mut i, "--source")?)),
            "--dest" => destinations.push(PathBuf::from(valeur(args, &mut i, "--dest")?)),
            "--nom" => nom = Some(valeur(args, &mut i, "--nom")?),
            "--empreinte" => {
                let v = valeur(args, &mut i, "--empreinte")?;
                algos = v
                    .split(',')
                    .map(|s| Algo::parse(s).ok_or_else(|| format!("empreinte inconnue : {s}")))
                    .collect::<Result<Vec<_>, _>>()?;
            }
            "--verifier-source" => verifier_source = true,
            autre => return Err(format!("option inconnue : {autre}")),
        }
        i += 1;
    }

    let source = source.ok_or("--source est obligatoire")?;
    if destinations.is_empty() {
        return Err("au moins une --dest est obligatoire".to_string());
    }

    let nom = nom.unwrap_or_else(|| {
        source
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "carte".to_string())
    });

    // Chaque destination reçoit un sous-dossier au nom du déchargement.
    let cibles: Vec<PathBuf> = destinations.iter().map(|d| d.join(&nom)).collect();

    println!("Source      : {}", source.display());
    for c in &cibles {
        println!("Destination : {}", c.display());
    }
    println!(
        "Empreinte   : {}",
        algos
            .iter()
            .map(|a| a.as_str())
            .collect::<Vec<_>>()
            .join(" + ")
    );
    println!();

    let opts = Options {
        source: source.clone(),
        destinations: cibles.clone(),
        algos: algos.clone(),
        verifier_source,
        nom: nom.clone(),
    };

    let mut dernier_pourcent = u64::MAX;
    let mut rapport = copy::decharger(&opts, &mut |p| match p {
        Progression::Demarrage {
            nb_fichiers,
            octets_total,
        } => {
            println!(
                "{nb_fichiers} fichiers, {} à copier\n",
                octets_lisibles(octets_total)
            );
        }
        Progression::Fichier {
            index,
            total,
            chemin,
        } => {
            println!("[{index}/{total}] {chemin}");
        }
        Progression::Octets { copies, total } => {
            let pct = copies
                .checked_mul(100)
                .and_then(|v| v.checked_div(total))
                .unwrap_or(100);
            if pct != dernier_pourcent {
                dernier_pourcent = pct;
                print!("\r        {pct} %   ");
                use std::io::Write;
                let _ = std::io::stdout().flush();
            }
        }
        Progression::Verification { .. } => {
            print!("\r        vérification…   ");
            use std::io::Write;
            let _ = std::io::stdout().flush();
        }
        Progression::Resultat {
            ok, verifications, ..
        } => {
            if ok {
                println!("\r        ✔ vérifié          ");
            } else {
                let detail = verifications
                    .iter()
                    .find(|v| !v.etat.ok())
                    .and_then(|v| v.erreur.clone())
                    .unwrap_or_else(|| "cause inconnue".to_string());
                println!("\r        ✖ ÉCHEC : {detail}");
            }
        }
    })
    .map_err(|e| e.to_string())?;

    // --- Dépôt du MHL et du rapport sur chaque destination.
    let horodatage = chrono::Local::now();
    for (i, cible) in cibles.iter().enumerate() {
        if !cible.is_dir() {
            continue;
        }
        let nom_mhl = mhl::nom_fichier(&nom, &horodatage);
        let chemin_mhl = cible.join(&nom_mhl);
        match mhl::ecrire(&rapport, i, &chemin_mhl) {
            Ok(_) => rapport.destinations[i].mhl = Some(nom_mhl),
            Err(e) => rapport
                .avertissements
                .push(format!("MHL non écrit sur {} : {e}", cible.display())),
        }
        let chemin_json = cible.join(format!("{nom}_rapport.json"));
        if let Err(e) = rapport.ecrire_json(&chemin_json) {
            rapport
                .avertissements
                .push(format!("rapport non écrit sur {} : {e}", cible.display()));
        }
    }

    afficher_bilan(&rapport);
    Ok(rapport.etat_global.ok())
}

fn afficher_bilan(rapport: &offload_core::report::Rapport) {
    println!("\n─────────────────────────────────────────────");
    println!(
        "{} fichiers · {} clips · {}",
        rapport.nb_fichiers,
        rapport.nb_clips,
        octets_lisibles(rapport.octets_total)
    );
    for d in &rapport.destinations {
        let symbole = if d.etat.ok() { "✔" } else { "✖" };
        println!(
            "{symbole} {} — {} vérifiés, {} en échec{}",
            d.dossier,
            d.fichiers_ok,
            d.fichiers_echec,
            d.erreur
                .as_ref()
                .map(|e| format!(" ({e})"))
                .unwrap_or_default()
        );
    }
    if let Some(s) = rapport.source_reverifiee {
        println!("{} relecture de la source", if s.ok() { "✔" } else { "✖" });
    }
    for a in &rapport.avertissements {
        println!("⚠ {a}");
    }
    println!(
        "\n{}",
        if rapport.etat_global.ok() {
            "✔ COPIE CONFORME"
        } else {
            "✖ COPIE NON CONFORME — ne pas effacer la carte"
        }
    );
}

fn commande_verifier(args: &[String]) -> Result<bool, String> {
    let mut dossier: Option<PathBuf> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--dossier" | "--path" => {
                dossier = Some(PathBuf::from(valeur(args, &mut i, "--dossier")?))
            }
            autre => return Err(format!("option inconnue : {autre}")),
        }
        i += 1;
    }
    let dossier = dossier.ok_or("--dossier est obligatoire")?;
    verifier_dossier(&dossier)
}

fn verifier_dossier(dossier: &Path) -> Result<bool, String> {
    let r = verify::controler_dossier(dossier).map_err(|e| e.to_string())?;

    for m in &r.mhls {
        println!("MHL : {m}");
    }
    for l in &r.lignes {
        if l.etat.ok() {
            println!("  \u{2714} {}", l.fichier);
        } else {
            println!(
                "  \u{2716} {} — {}",
                l.fichier,
                l.detail
                    .clone()
                    .unwrap_or_else(|| "cause inconnue".to_string())
            );
        }
    }

    println!("\n─────────────────────────────────────────────");
    println!(
        "{} fichiers listés, {} en échec, {} absents",
        r.lignes.len(),
        r.nb_echecs(),
        r.absents.len()
    );
    if !r.en_trop.is_empty() {
        println!(
            "{} fichiers présents mais absents du MHL :",
            r.en_trop.len()
        );
        for f in r.en_trop.iter().take(20) {
            println!("  \u{26a0} {f}");
        }
    }
    if !r.residus.is_empty() {
        println!("{} copies interrompues (.offload-part) :", r.residus.len());
        for f in r.residus.iter().take(20) {
            println!("  \u{26a0} {f}");
        }
    }

    println!(
        "\n{}",
        if r.etat.ok() {
            "\u{2714} DOSSIER CONFORME"
        } else {
            "\u{2716} DOSSIER NON CONFORME"
        }
    );
    Ok(r.etat.ok())
}
