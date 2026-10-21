//! SuperLoc Offload en ligne de commande.
//!
//! Codes de sortie : 0 bon à formater, 1 copie conforme mais ne pas formater
//! encore, 2 échec, 3 annulé, 4 refusé avant copie ou erreur d'usage.

use offload_core::gabarit::{self, Valeurs};
use offload_core::hash::{self, Algo};
use offload_core::langue::{self, Langue};
use offload_core::moteur::{decharger, Annulation, Evenement, Options, Phase};
use offload_core::rapport::{ModeVerif, Verdict};
use offload_core::{verification, volume, OUTIL, VERSION};
use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

const AIDE: &str = "\
SuperLoc Offload — copie et vérification de rushs

UTILISATION
  offload copier <CARTE> --dest <DOSSIER> [--dest <DOSSIER>…] [options]
  offload verifier <DOSSIER>
  offload volumes
  offload version

COPIER
  --dest <DOSSIER>     Dossier de base sur un disque (répétable). Le dossier du
                       déchargement y est créé d'après le gabarit.
  --exact              Les --dest sont les dossiers finaux (pas de gabarit).
  --nom <NOM>          Nom de la carte (défaut : nom du volume ou du dossier).
  --gabarit <G>        Défaut « {date}_{carte} ». Jetons : {carte} {date}
                       {projet} {jour} {camera} {operateur}. « / » = sous-dossier.
  --projet, --jour, --camera, --operateur, --email, --lieu, --commentaire <TEXTE>
  --algo <LISTE>       xxh128,xxh64 (défaut) ; aussi xxh3, md5, sha1, c4.
  --mode <MODE>        complet (défaut) | carte (relit aussi la carte) | taille
  --sans-mhl1          Ne pas écrire de MHL v1.1 en plus de l'ASC MHL.
  --json               Écrit le rapport JSON sur la sortie standard.
  --langue <fr|en>

CODES DE SORTIE
  0 bon à formater · 1 conforme, ne pas formater encore · 2 échec
  3 annulé · 4 refusé avant copie
";

struct Args {
    liste: Vec<String>,
}

impl Args {
    fn valeur(&mut self, nom: &str) -> Option<String> {
        let i = self.liste.iter().position(|a| a == nom)?;
        if i + 1 >= self.liste.len() {
            return None;
        }
        let v = self.liste.remove(i + 1);
        self.liste.remove(i);
        Some(v)
    }
    fn valeurs(&mut self, nom: &str) -> Vec<String> {
        let mut v = Vec::new();
        while let Some(x) = self.valeur(nom) {
            v.push(x);
        }
        v
    }
    fn drapeau(&mut self, nom: &str) -> bool {
        match self.liste.iter().position(|a| a == nom) {
            Some(i) => {
                self.liste.remove(i);
                true
            }
            None => false,
        }
    }
}

fn erreur_usage(m: &str) -> ExitCode {
    eprintln!("{m}\n\n{AIDE}");
    ExitCode::from(4)
}

fn main() -> ExitCode {
    let mut args = Args {
        liste: std::env::args().skip(1).collect(),
    };
    if args.liste.is_empty() || args.drapeau("--help") || args.drapeau("-h") {
        println!("{AIDE}");
        return ExitCode::SUCCESS;
    }
    let commande = args.liste.remove(0);
    let l = args
        .valeur("--langue")
        .map(|x| Langue::depuis(&x))
        .unwrap_or_default();
    match commande.as_str() {
        "copier" | "copy" => copier(args, l),
        "verifier" | "vérifier" | "verify" => verifier(args, l),
        "volumes" => {
            for v in volume::lister() {
                println!(
                    "{:<28} {:<8} {:>10} libres  {}{}",
                    v.point_montage,
                    v.systeme_fichiers,
                    langue::octets(v.libre, l),
                    v.nom,
                    if v.amovible { "  (amovible)" } else { "" }
                );
            }
            ExitCode::SUCCESS
        }
        "version" | "--version" => {
            println!("{OUTIL} {VERSION}");
            ExitCode::SUCCESS
        }
        autre => erreur_usage(&format!("Commande inconnue : {autre}")),
    }
}

fn eveil() -> Option<keepawake::KeepAwake> {
    keepawake::Builder::default()
        .idle(true)
        .sleep(true)
        .reason("Copie de rushs en cours")
        .app_name(OUTIL)
        .app_reverse_domain("fr.superloc.offload")
        .create()
        .ok()
}

fn copier(mut args: Args, l: Langue) -> ExitCode {
    let bases: Vec<PathBuf> = args
        .valeurs("--dest")
        .into_iter()
        .map(PathBuf::from)
        .collect();
    let exact = args.drapeau("--exact");
    let json = args.drapeau("--json");
    let sans_mhl1 = args.drapeau("--sans-mhl1");
    let nom = args.valeur("--nom");
    let modele = args
        .valeur("--gabarit")
        .unwrap_or_else(|| gabarit::PAR_DEFAUT.into());
    let mut valeurs = Valeurs {
        projet: args.valeur("--projet").unwrap_or_default(),
        jour: args.valeur("--jour").unwrap_or_default(),
        camera: args.valeur("--camera").unwrap_or_default(),
        operateur: args.valeur("--operateur").unwrap_or_default(),
        date: chrono::Local::now().format("%Y-%m-%d").to_string(),
        ..Default::default()
    };
    let email = args.valeur("--email").unwrap_or_default();
    let lieu = args.valeur("--lieu").unwrap_or_default();
    let commentaire = args.valeur("--commentaire").unwrap_or_default();
    let algos = match args.valeur("--algo") {
        None => hash::PAR_DEFAUT.to_vec(),
        Some(s) => {
            let mut v = Vec::new();
            for x in s.split(',') {
                match Algo::depuis_nom(x) {
                    Some(a) => v.push(a),
                    None => return erreur_usage(&format!("Algorithme inconnu : {x}")),
                }
            }
            v
        }
    };
    let mode = match args.valeur("--mode").as_deref() {
        None | Some("complet") | Some("full") => ModeVerif::Complet,
        Some("carte") | Some("card") => ModeVerif::CompletCarte,
        Some("taille") | Some("size") => ModeVerif::Taille,
        Some(x) => return erreur_usage(&format!("Mode inconnu : {x}")),
    };
    if let Some(x) = args.liste.iter().find(|a| a.starts_with("--")) {
        return erreur_usage(&format!("Option inconnue : {x}"));
    }
    let Some(source) = args.liste.first().map(PathBuf::from) else {
        return erreur_usage("Carte source manquante.");
    };
    if bases.is_empty() {
        return erreur_usage("Au moins une --dest est nécessaire.");
    }

    let mut opts = Options::new(&source, Vec::new());
    if let Some(n) = nom {
        opts.nom = n;
    }
    valeurs.carte = opts.nom.clone();
    opts.destinations = if exact {
        bases
    } else {
        // Jamais d'écrasement : un dossier déjà pris reçoit un suffixe.
        let rel = gabarit::nom_libre(&bases, &gabarit::rendre(&modele, &valeurs));
        gabarit::dossiers(&bases, &rel)
    };
    opts.algos = algos;
    opts.mode = mode;
    opts.mhl_v1 = !sans_mhl1;
    opts.operateur = valeurs.operateur.clone();
    opts.email = email;
    opts.projet = valeurs.projet.clone();
    opts.lieu = lieu;
    opts.commentaire = commentaire;
    opts.langue = l;
    opts.annulation = Annulation::default();
    let jeton = opts.annulation.clone();
    let _ = ctrlc::set_handler(move || {
        eprintln!("\nAnnulation demandée…");
        jeton.annuler();
    });

    for (i, d) in opts.destinations.iter().enumerate() {
        eprintln!("→ {} : {}", i + 1, d.display());
    }
    let _eveil = eveil();
    let mut affiche = |e: Evenement| match e {
        Evenement::Phase { phase } => {
            let t = match phase {
                Phase::Inventaire => l.t("Inventaire de la carte…", "Scanning card…"),
                Phase::Controle => l.t("Contrôles…", "Checks…"),
                Phase::Copie => l.t("Copie et vérification…", "Copy and verification…"),
                Phase::RelectureCarte => l.t("\nRelecture de la carte…", "\nRe-reading card…"),
                Phase::Rapports => l.t("\nManifestes et rapports…", "\nManifests and reports…"),
                Phase::Termine => "",
            };
            if !t.is_empty() {
                eprintln!("{t}");
            }
        }
        Evenement::Debut {
            nb_fichiers,
            octets,
            nb_clips,
            ..
        } => eprintln!(
            "{nb_fichiers} fichiers, {nb_clips} clips, {}",
            langue::octets(octets, l)
        ),
        Evenement::Avancement(a) => {
            let pct = if a.total > 0 {
                a.lus as f64 * 100.0 / a.total as f64
            } else {
                100.0
            };
            let dests: Vec<String> = a
                .destinations
                .iter()
                .enumerate()
                .map(|(i, d)| {
                    if d.en_panne {
                        format!("{}: PANNE", i + 1)
                    } else {
                        format!("{}: {}", i + 1, langue::debit(d.debit, l))
                    }
                })
                .collect();
            let reste = a
                .restant_s
                .map(|s| format!(" · reste {}", langue::duree(s, l)))
                .unwrap_or_default();
            eprint!(
                "\r{pct:5.1} %  {}/{}  lecture {}  [{}]{reste}        ",
                a.index + 1,
                a.nb_fichiers,
                langue::debit(a.debit_lecture, l),
                dests.join(" ")
            );
            let _ = std::io::stderr().flush();
        }
        Evenement::Copie {
            destination,
            chemin,
            erreur: Some(x),
            ..
        } => eprintln!("\n✗ {chemin} → {} : {}", destination + 1, x.message(l)),
        Evenement::Source { chemin, erreur, .. } => {
            eprintln!("\n✗ {chemin} (carte) : {}", erreur.message(l))
        }
        _ => {}
    };

    let r = match decharger(&opts, &mut affiche) {
        Ok(r) => r,
        Err(refus) => {
            eprintln!("{}", l.t("Copie refusée :", "Copy refused:"));
            for e in &refus.erreurs {
                eprintln!("  ✗ {}\n    {}", e.message(l), e.action(l));
            }
            return ExitCode::from(4);
        }
    };
    let noms = r.noms_destinations();
    eprintln!("\n\n{}", r.verdict.titre(l));
    for x in &r.remarques {
        eprintln!("  • {}", x.texte(l, &noms));
    }
    for d in &r.destinations {
        eprintln!("  {} → {}/_offload/rapport.html", d.dossier, d.dossier);
    }
    if json {
        println!("{}", r.json());
    }
    ExitCode::from(match r.verdict {
        Verdict::BonAFormater => 0,
        Verdict::ConformeNePasFormater => 1,
        Verdict::Echec => 2,
        Verdict::Annule => 3,
    })
}

fn verifier(mut args: Args, l: Langue) -> ExitCode {
    let json = args.drapeau("--json");
    let Some(dossier) = args.liste.first().map(PathBuf::from) else {
        return erreur_usage("Dossier à vérifier manquant.");
    };
    let jeton = Annulation::default();
    let j = jeton.clone();
    let _ = ctrlc::set_handler(move || j.annuler());
    let _eveil = eveil();
    let b = match verification::verifier(&dossier, &jeton, &mut |p| {
        eprint!(
            "\r{}/{}  {}            ",
            p.index + 1,
            p.nb_fichiers,
            langue::octets(p.octets, l)
        );
    }) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("✗ {}\n  {}", e.message(l), e.action(l));
            return ExitCode::from(4);
        }
    };
    eprintln!();
    if json {
        println!("{}", serde_json::to_string_pretty(&b).unwrap_or_default());
    }
    for p in &b.problemes {
        eprintln!("✗ {} : {}", p.chemin, p.erreur.message(l));
    }
    for x in &b.inattendus {
        eprintln!("? {x} : {}", l.t("absent du manifeste", "not in manifest"));
    }
    if b.conforme() {
        eprintln!(
            "{} ({} {}, {})",
            l.t("CONFORME", "VERIFIED"),
            b.conformes,
            l.t("fichiers", "files"),
            b.format
        );
        ExitCode::SUCCESS
    } else if b.annule {
        ExitCode::from(3)
    } else {
        eprintln!("{}", l.t("NON CONFORME", "NOT VERIFIED"));
        ExitCode::from(2)
    }
}
