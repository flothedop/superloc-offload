//! ASC MHL v2.0 (spécification ASC MHL v1.0 du 15 mars 2022).
//!
//! Sur chaque destination : un dossier `ascmhl/` à la racine du déchargement,
//! contenant une « génération » (`0001_<dossier>_<date>_<heure>Z.mhl`) et la
//! chaîne `ascmhl_chain.xml`, qui scelle chaque génération par son empreinte
//! C4. Les empreintes de dossiers (contenu et structure) sont calculées comme
//! l'outil de référence de l'ASC, ce que vérifient les tests.

use crate::erreur::{Code, Erreur};
use crate::hash::{self, Algo, Empreinte};
use crate::xml::{self, esc};
use chrono::{DateTime, SecondsFormat, Utc};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

pub const DOSSIER: &str = "ascmhl";
pub const CHAINE: &str = "ascmhl_chain.xml";
const NS: &str = "urn:ASC:MHL:v2.0";
const NS_CHAINE: &str = "urn:ASC:MHL:DIRECTORY:v2.0";

#[derive(Clone, Debug)]
pub struct Entree {
    /// Chemin relatif à la racine, séparateurs `/`.
    pub chemin: String,
    pub taille: u64,
    pub modifie: Option<DateTime<Utc>>,
    pub cree: Option<DateTime<Utc>>,
    pub empreintes: Vec<Empreinte>,
    pub date_empreinte: DateTime<Utc>,
    /// `original` à la création, `verified` si l'empreinte a été recalculée
    /// et trouvée identique à une génération précédente.
    pub action: &'static str,
}

#[derive(Clone, Debug)]
pub struct Dossier {
    pub chemin: String,
    pub modifie: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, Default)]
pub struct Createur {
    pub date: DateTime<Utc>,
    pub machine: String,
    pub outil: String,
    pub version: String,
    pub auteur: Option<Auteur>,
    pub lieu: Option<String>,
    pub commentaire: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct Auteur {
    pub nom: String,
    pub email: Option<String>,
    pub telephone: Option<String>,
    pub role: Option<String>,
}

fn date(d: &DateTime<Utc>) -> String {
    d.to_rfc3339_opts(SecondsFormat::Secs, true)
}

fn date_precise(d: &DateTime<Utc>) -> String {
    d.to_rfc3339_opts(SecondsFormat::Millis, true)
}

/// Nom d'une génération : `0001_<dossier>_AAAA-MM-JJ_HHMMSSZ.mhl`.
pub fn nom_generation(numero: u32, dossier: &str, debut: &DateTime<Utc>) -> String {
    format!(
        "{numero:04}_{dossier}_{}Z.mhl",
        debut.format("%Y-%m-%d_%H%M%S")
    )
}

// ------------------------------------------------------------------------
// Empreintes de dossiers
// ------------------------------------------------------------------------

/// Octets d'une empreinte telle qu'écrite (hex, ou C4 en base 58).
fn octets(algo: Algo, valeur: &str) -> Vec<u8> {
    if algo == Algo::C4 {
        return c4_vers_octets(valeur).unwrap_or_default();
    }
    hash::depuis_hex(valeur).unwrap_or_default()
}

fn c4_vers_octets(c4: &str) -> Option<Vec<u8>> {
    const A: &str = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
    let mut n = vec![0u8; 64];
    for ch in c4.get(2..)?.chars() {
        let mut retenue = A.find(ch)? as u32;
        for o in n.iter_mut().rev() {
            let v = u32::from(*o) * 58 + retenue;
            *o = (v & 0xff) as u8;
            retenue = v >> 8;
        }
    }
    Some(n)
}

/// Empreinte d'une liste d'empreintes : triées comme chaînes, puis leurs
/// octets concaténés. Liste vide : empreinte de zéro octet.
fn empreinte_de_liste(algo: Algo, mut liste: Vec<String>) -> String {
    liste.sort();
    let mut h = hash::Hacheur::new(algo);
    for e in &liste {
        h.update(&octets(algo, e));
    }
    h.finir()
}

#[derive(Clone, Debug)]
pub struct EmpreintesDossier {
    pub contenu: String,
    pub structure: String,
}

/// Empreintes de contenu et de structure de chaque dossier (racine incluse,
/// clé ""), pour un algorithme.
pub fn empreintes_dossiers(
    algo: Algo,
    fichiers: &[Entree],
    dossiers: &[Dossier],
) -> BTreeMap<String, EmpreintesDossier> {
    let parent = |c: &str| {
        c.rsplit_once('/')
            .map(|(p, _)| p.to_string())
            .unwrap_or_default()
    };
    let feuille = |c: &str| c.rsplit('/').next().unwrap_or(c).to_string();

    let mut tous: Vec<String> = dossiers.iter().map(|d| d.chemin.clone()).collect();
    tous.push(String::new());
    // Les plus profonds d'abord : un dossier se calcule après ses enfants.
    tous.sort_by_key(|d| {
        std::cmp::Reverse(if d.is_empty() {
            0
        } else {
            d.matches('/').count() + 1
        })
    });
    tous.dedup();

    let mut resultat: BTreeMap<String, EmpreintesDossier> = BTreeMap::new();
    for d in &tous {
        let mut contenus = Vec::new();
        let mut structures = Vec::new();
        for f in fichiers.iter().filter(|f| parent(&f.chemin) == *d) {
            let Some(e) = f.empreintes.iter().find(|e| e.algo == algo) else {
                continue;
            };
            contenus.push(e.valeur.clone());
            let mut nom = feuille(&f.chemin).into_bytes();
            nom.extend(octets(algo, &e.valeur));
            structures.push(hash::de_donnees(algo, &nom));
        }
        for sous in tous
            .iter()
            .filter(|s| !s.is_empty() && parent(s) == *d && *s != d)
        {
            if let Some(e) = resultat.get(sous) {
                contenus.push(e.contenu.clone());
                let mut nom = feuille(sous).into_bytes();
                nom.extend(octets(algo, &e.structure));
                structures.push(hash::de_donnees(algo, &nom));
            }
        }
        resultat.insert(
            d.clone(),
            EmpreintesDossier {
                contenu: empreinte_de_liste(algo, contenus),
                structure: empreinte_de_liste(algo, structures),
            },
        );
    }
    resultat
}

// ------------------------------------------------------------------------
// Écriture
// ------------------------------------------------------------------------

/// Construit le texte d'une génération.
pub fn generation(
    createur: &Createur,
    process: &str,
    fichiers: &[Entree],
    dossiers: &[Dossier],
    motifs_ignores: &[String],
    algos_dossiers: &[Algo],
) -> String {
    let mut x = String::new();
    let l = |x: &mut String, s: &str| {
        x.push_str(s);
        x.push('\n');
    };
    l(&mut x, r#"<?xml version="1.0" encoding="UTF-8"?>"#);
    l(&mut x, &format!(r#"<hashlist version="2.0" xmlns="{NS}">"#));
    l(&mut x, "  <creatorinfo>");
    l(
        &mut x,
        &format!("    <creationdate>{}</creationdate>", date(&createur.date)),
    );
    l(
        &mut x,
        &format!("    <hostname>{}</hostname>", esc(&createur.machine)),
    );
    l(
        &mut x,
        &format!(
            r#"    <tool version="{}">{}</tool>"#,
            esc(&createur.version),
            esc(&createur.outil)
        ),
    );
    if let Some(a) = &createur.auteur {
        if !a.nom.trim().is_empty() {
            let mut attrs = String::new();
            for (k, v) in [
                ("email", &a.email),
                ("phone", &a.telephone),
                ("role", &a.role),
            ] {
                if let Some(v) = v.as_ref().filter(|v| !v.trim().is_empty()) {
                    attrs.push_str(&format!(r#" {k}="{}""#, esc(v)));
                }
            }
            l(
                &mut x,
                &format!("    <author{attrs}>{}</author>", esc(&a.nom)),
            );
        }
    }
    if let Some(v) = createur.lieu.as_ref().filter(|v| !v.trim().is_empty()) {
        l(&mut x, &format!("    <location>{}</location>", esc(v)));
    }
    if let Some(v) = createur
        .commentaire
        .as_ref()
        .filter(|v| !v.trim().is_empty())
    {
        l(&mut x, &format!("    <comment>{}</comment>", esc(v)));
    }
    l(&mut x, "  </creatorinfo>");

    let calculs: Vec<(Algo, BTreeMap<String, EmpreintesDossier>)> = algos_dossiers
        .iter()
        .map(|a| (*a, empreintes_dossiers(*a, fichiers, dossiers)))
        .collect();
    let hashdate = date_precise(&createur.date);

    l(&mut x, "  <processinfo>");
    l(&mut x, &format!("    <process>{process}</process>"));
    if !calculs.is_empty() {
        l(&mut x, "    <roothash>");
        for partie in ["content", "structure"] {
            l(&mut x, &format!("      <{partie}>"));
            for (algo, m) in &calculs {
                if let Some(e) = m.get("") {
                    let v = if partie == "content" {
                        &e.contenu
                    } else {
                        &e.structure
                    };
                    l(
                        &mut x,
                        &format!(
                            r#"        <{0} hashdate="{hashdate}">{v}</{0}>"#,
                            algo.nom()
                        ),
                    );
                }
            }
            l(&mut x, &format!("      </{partie}>"));
        }
        l(&mut x, "    </roothash>");
    }
    if !motifs_ignores.is_empty() {
        l(&mut x, "    <ignore>");
        for m in motifs_ignores {
            l(&mut x, &format!("      <pattern>{}</pattern>", esc(m)));
        }
        l(&mut x, "    </ignore>");
    }
    l(&mut x, "  </processinfo>");

    l(&mut x, "  <hashes>");
    for f in fichiers {
        let mut attrs = format!(r#" size="{}""#, f.taille);
        if let Some(c) = &f.cree {
            attrs.push_str(&format!(r#" creationdate="{}""#, date(c)));
        }
        if let Some(m) = &f.modifie {
            attrs.push_str(&format!(r#" lastmodificationdate="{}""#, date(m)));
        }
        l(&mut x, "    <hash>");
        l(
            &mut x,
            &format!("      <path{attrs}>{}</path>", esc(&f.chemin)),
        );
        // Ordre imposé par le schéma : c4, md5, sha1, xxh128, xxh3, xxh64.
        let mut e = f.empreintes.clone();
        e.sort_by_key(|e| e.algo);
        for e in &e {
            l(
                &mut x,
                &format!(
                    r#"      <{0} action="{1}" hashdate="{2}">{3}</{0}>"#,
                    e.algo.nom(),
                    f.action,
                    date_precise(&f.date_empreinte),
                    e.valeur
                ),
            );
        }
        l(&mut x, "    </hash>");
    }
    for d in dossiers {
        let mut attrs = String::new();
        if let Some(m) = &d.modifie {
            attrs.push_str(&format!(r#" lastmodificationdate="{}""#, date(m)));
        }
        l(&mut x, "    <directoryhash>");
        l(
            &mut x,
            &format!("      <path{attrs}>{}</path>", esc(&d.chemin)),
        );
        for partie in ["content", "structure"] {
            if calculs.is_empty() {
                break;
            }
            l(&mut x, &format!("      <{partie}>"));
            for (algo, m) in &calculs {
                if let Some(e) = m.get(&d.chemin) {
                    let v = if partie == "content" {
                        &e.contenu
                    } else {
                        &e.structure
                    };
                    l(
                        &mut x,
                        &format!(
                            r#"        <{0} hashdate="{hashdate}">{v}</{0}>"#,
                            algo.nom()
                        ),
                    );
                }
            }
            l(&mut x, &format!("      </{partie}>"));
        }
        l(&mut x, "    </directoryhash>");
    }
    l(&mut x, "  </hashes>");
    l(&mut x, "</hashlist>");
    x
}

/// Texte de la chaîne pour une liste de (nom de génération, C4).
pub fn chaine(generations: &[(String, String)]) -> String {
    let mut x = String::new();
    x.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    x.push_str(&format!("<ascmhldirectory xmlns=\"{NS_CHAINE}\">\n"));
    for (i, (nom, c4)) in generations.iter().enumerate() {
        x.push_str(&format!("  <hashlist sequencenr=\"{}\">\n", i + 1));
        x.push_str(&format!("    <path>{}</path>\n", esc(nom)));
        x.push_str(&format!("    <c4>{c4}</c4>\n"));
        x.push_str("  </hashlist>\n");
    }
    x.push_str("</ascmhldirectory>\n");
    x
}

fn ecrire_fichier(chemin: &Path, contenu: &[u8]) -> std::io::Result<()> {
    let mut f = crate::es::creer_exclusif(chemin)?;
    f.write_all(contenu)?;
    crate::es::synchroniser(&f)?;
    Ok(())
}

/// Écrit la première génération et la chaîne dans `racine/ascmhl/`.
/// Renvoie le chemin relatif de la génération.
#[allow(clippy::too_many_arguments)]
pub fn ecrire(
    racine: &Path,
    debut: &DateTime<Utc>,
    createur: &Createur,
    fichiers: &[Entree],
    dossiers: &[Dossier],
    motifs_ignores: &[String],
    algos_dossiers: &[Algo],
) -> Result<String, Erreur> {
    let dossier = racine.join(DOSSIER);
    let err = |e: std::io::Error| Erreur::io(Code::EcritureImpossible, dossier.display(), &e);
    std::fs::create_dir_all(&dossier).map_err(err)?;
    let feuille = racine
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "offload".into());
    let nom = nom_generation(1, &feuille, debut);
    let texte = generation(
        createur,
        "transfer",
        fichiers,
        dossiers,
        motifs_ignores,
        algos_dossiers,
    );
    ecrire_fichier(&dossier.join(&nom), texte.as_bytes()).map_err(err)?;
    let c4 = hash::de_donnees(Algo::C4, texte.as_bytes());
    ecrire_fichier(
        &dossier.join(CHAINE),
        chaine(&[(nom.clone(), c4)]).as_bytes(),
    )
    .map_err(err)?;
    Ok(format!("{DOSSIER}/{nom}"))
}

// ------------------------------------------------------------------------
// Lecture
// ------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct Generation {
    pub chemin: PathBuf,
    pub numero: u32,
    pub entrees: Vec<Entree>,
    pub ignores: Vec<String>,
    pub outil: String,
    pub date: Option<DateTime<Utc>>,
}

fn lire_date(s: Option<&str>) -> Option<DateTime<Utc>> {
    s.and_then(|s| DateTime::parse_from_rfc3339(s.trim()).ok())
        .map(|d| d.with_timezone(&Utc))
}

/// Lit un historique ASC MHL : vérifie la chaîne (C4 de chaque génération)
/// et renvoie les générations dans l'ordre.
pub fn lire_historique(racine: &Path) -> Result<Vec<Generation>, Erreur> {
    let dossier = racine.join(DOSSIER);
    let chemin_chaine = dossier.join(CHAINE);
    let texte = std::fs::read_to_string(&chemin_chaine)
        .map_err(|e| Erreur::io(Code::ManifesteAbsent, chemin_chaine.display(), &e))?;
    let arbre = xml::lire(&texte).map_err(|e| {
        Erreur::new(
            Code::ManifesteIllisible,
            format!("{} : {e}", chemin_chaine.display()),
        )
    })?;
    let mut generations = Vec::new();
    for h in arbre.enfants("hashlist") {
        let numero: u32 = h
            .attribut("sequencenr")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let nom = h.texte_de("path").unwrap_or_default().to_string();
        let attendu = h.texte_de("c4").unwrap_or_default().to_string();
        let chemin = dossier.join(&nom);
        let octets = std::fs::read(&chemin)
            .map_err(|e| Erreur::io(Code::ManifesteAbsent, chemin.display(), &e))?;
        let c4 = hash::de_donnees(Algo::C4, &octets);
        if c4 != attendu {
            return Err(Erreur::new(
                Code::ManifesteModifie,
                chemin.display().to_string(),
            ));
        }
        let texte = String::from_utf8(octets).map_err(|e| {
            Erreur::new(
                Code::ManifesteIllisible,
                format!("{} : {e}", chemin.display()),
            )
        })?;
        let mut g = lire_generation(&texte).map_err(|e| {
            Erreur::new(
                Code::ManifesteIllisible,
                format!("{} : {e}", chemin.display()),
            )
        })?;
        g.chemin = chemin;
        g.numero = numero;
        generations.push(g);
    }
    if generations.is_empty() {
        return Err(Erreur::new(
            Code::ManifesteAbsent,
            dossier.display().to_string(),
        ));
    }
    generations.sort_by_key(|g| g.numero);
    Ok(generations)
}

pub fn lire_generation(texte: &str) -> Result<Generation, String> {
    let r = xml::lire(texte)?;
    if r.nom != "hashlist" {
        return Err(format!("racine inattendue <{}>", r.nom));
    }
    let createur = r.enfant("creatorinfo");
    let mut g = Generation {
        chemin: PathBuf::new(),
        numero: 0,
        entrees: Vec::new(),
        ignores: r
            .enfant("processinfo")
            .and_then(|p| p.enfant("ignore"))
            .map(|i| {
                i.enfants("pattern")
                    .map(|p| p.texte.trim().to_string())
                    .collect()
            })
            .unwrap_or_default(),
        outil: createur
            .and_then(|c| c.texte_de("tool"))
            .unwrap_or_default()
            .to_string(),
        date: lire_date(createur.and_then(|c| c.texte_de("creationdate"))),
    };
    if let Some(h) = r.enfant("hashes") {
        for e in h.enfants("hash") {
            let Some(p) = e.enfant("path") else { continue };
            let mut empreintes = Vec::new();
            let mut action = "original";
            let mut date_empreinte = None;
            for x in &e.enfants {
                if let Some(algo) = Algo::depuis_nom(&x.nom).filter(|_| x.nom != "path") {
                    if x.attribut("action") == Some("failed") {
                        action = "failed";
                    }
                    date_empreinte = date_empreinte.or(lire_date(x.attribut("hashdate")));
                    empreintes.push(Empreinte {
                        algo,
                        valeur: x.texte.trim().to_string(),
                    });
                }
            }
            g.entrees.push(Entree {
                chemin: p.texte.trim().to_string(),
                taille: p.attribut("size").and_then(|s| s.parse().ok()).unwrap_or(0),
                modifie: lire_date(p.attribut("lastmodificationdate")),
                cree: lire_date(p.attribut("creationdate")),
                empreintes,
                date_empreinte: date_empreinte.unwrap_or_default(),
                action,
            });
        }
    }
    Ok(g)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entree(chemin: &str, donnees: &[u8]) -> Entree {
        Entree {
            chemin: chemin.into(),
            taille: donnees.len() as u64,
            modifie: None,
            cree: None,
            empreintes: vec![
                Empreinte {
                    algo: Algo::Xxh128,
                    valeur: hash::de_donnees(Algo::Xxh128, donnees),
                },
                Empreinte {
                    algo: Algo::Xxh64,
                    valeur: hash::de_donnees(Algo::Xxh64, donnees),
                },
            ],
            date_empreinte: Utc::now(),
            action: "original",
        }
    }

    /// Valeurs produites par l'outil de référence de l'ASC (ascmhl 1.2) sur
    /// l'arborescence : A001.RDC/A_001.R3D = "abc", vide/ (dossier vide),
    /// zero.txt (0 octet).
    #[test]
    fn empreintes_de_dossiers_identiques_a_la_reference() {
        let fichiers = vec![
            entree("A001.RDC/A_001.R3D", b"abc"),
            entree("zero.txt", b""),
        ];
        let dossiers = vec![
            Dossier {
                chemin: "A001.RDC".into(),
                modifie: None,
            },
            Dossier {
                chemin: "vide".into(),
                modifie: None,
            },
        ];
        let x128 = empreintes_dossiers(Algo::Xxh128, &fichiers, &dossiers);
        assert_eq!(x128["A001.RDC"].contenu, "f758394eebabb2bb51ee911f78f31836");
        assert_eq!(
            x128["A001.RDC"].structure,
            "9755f4bc6d4b31b7ffc076b356ad9a76"
        );
        assert_eq!(x128["vide"].contenu, "99aa06d3014798d86001c324468d497f");
        assert_eq!(x128[""].contenu, "7f0836fb4237d642134b7096225885ea");
        assert_eq!(x128[""].structure, "fde2faef7b8721362fd1b0a218560293");
        let x64 = empreintes_dossiers(Algo::Xxh64, &fichiers, &dossiers);
        assert_eq!(x64[""].contenu, "84b71fdf71109ca0");
        assert_eq!(x64[""].structure, "be46a16eaa3ed0ee");
        assert_eq!(x64["A001.RDC"].structure, "af4b68313c330e0d");
    }

    #[test]
    fn c4_aller_retour() {
        let c4 = hash::de_donnees(Algo::C4, b"abc");
        let o = c4_vers_octets(&c4).unwrap();
        assert_eq!(hash::c4_depuis_sha512(&o), c4);
    }

    #[test]
    fn ecrit_puis_relit_une_generation_scellee() {
        let d = tempfile::tempdir().unwrap();
        let racine = d.path().join("A001");
        std::fs::create_dir(&racine).unwrap();
        let debut = Utc::now();
        let createur = Createur {
            date: debut,
            machine: "poste".into(),
            outil: "SuperLoc Offload".into(),
            version: "0.2.0".into(),
            auteur: Some(Auteur {
                nom: "Flo & Co".into(),
                role: Some("DIT".into()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let f = vec![entree("Clip/<é>.mov", b"abc")];
        let rel = ecrire(
            &racine,
            &debut,
            &createur,
            &f,
            &[],
            &["ascmhl/".into()],
            &[Algo::Xxh128],
        )
        .unwrap();
        assert!(rel.starts_with("ascmhl/0001_A001_"));
        let h = lire_historique(&racine).unwrap();
        assert_eq!(h.len(), 1);
        assert_eq!(h[0].entrees[0].chemin, "Clip/<é>.mov");
        assert_eq!(h[0].entrees[0].empreintes.len(), 2);
        assert_eq!(h[0].ignores, vec!["ascmhl/"]);

        // Toute modification du manifeste est détectée par la chaîne.
        let g = &h[0].chemin;
        let t = std::fs::read_to_string(g).unwrap().replace("abc", "abd");
        std::fs::write(g, t + " ").unwrap();
        assert_eq!(
            lire_historique(&racine).unwrap_err().code,
            Code::ManifesteModifie
        );
    }
}
