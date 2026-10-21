//! Lecture XML minimale, en arbre, pour les manifestes MHL.

use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

#[derive(Clone, Debug, Default)]
pub struct Noeud {
    /// Nom local, sans préfixe d'espace de noms.
    pub nom: String,
    pub attributs: Vec<(String, String)>,
    pub texte: String,
    pub enfants: Vec<Noeud>,
}

impl Noeud {
    pub fn attribut(&self, nom: &str) -> Option<&str> {
        self.attributs
            .iter()
            .find(|(n, _)| n == nom)
            .map(|(_, v)| v.as_str())
    }

    pub fn enfant(&self, nom: &str) -> Option<&Noeud> {
        self.enfants.iter().find(|e| e.nom == nom)
    }

    pub fn enfants<'a>(&'a self, nom: &'a str) -> impl Iterator<Item = &'a Noeud> + 'a {
        self.enfants.iter().filter(move |e| e.nom == nom)
    }

    pub fn texte_de(&self, nom: &str) -> Option<&str> {
        self.enfant(nom).map(|e| e.texte.trim())
    }
}

fn local(s: &str) -> String {
    match s.rsplit_once(':') {
        Some((_, l)) => l.to_string(),
        None => s.to_string(),
    }
}

fn ouvrir(e: &BytesStart) -> Result<Noeud, String> {
    let mut n = Noeud {
        nom: local(e.name().as_ref()),
        ..Default::default()
    };
    for a in e.attributes() {
        let a = a.map_err(|e| e.to_string())?;
        let valeur = a
            .normalized_value(quick_xml::XmlVersion::Implicit1_0)
            .map_err(|e| e.to_string())?
            .to_string();
        n.attributs.push((local(a.key.as_ref()), valeur));
    }
    Ok(n)
}

/// Analyse un document et renvoie sa racine.
pub fn lire(contenu: &str) -> Result<Noeud, String> {
    let mut lecteur = Reader::from_str(contenu);
    let mut pile: Vec<Noeud> = Vec::new();
    let mut racine: Option<Noeud> = None;
    loop {
        match lecteur.read_event().map_err(|e| e.to_string())? {
            Event::Start(e) => pile.push(ouvrir(&e)?),
            Event::Empty(e) => {
                let n = ouvrir(&e)?;
                match pile.last_mut() {
                    Some(p) => p.enfants.push(n),
                    None => racine = Some(n),
                }
            }
            Event::Text(t) => {
                if let Some(p) = pile.last_mut() {
                    p.texte.push_str(&t.xml10_content());
                }
            }
            Event::CData(t) => {
                if let Some(p) = pile.last_mut() {
                    p.texte.push_str(t.as_ref());
                }
            }
            Event::GeneralRef(r) => {
                let c = match r.resolve_char_ref().map_err(|e| e.to_string())? {
                    Some(c) => c,
                    None => match r.as_ref() {
                        "amp" => '&',
                        "lt" => '<',
                        "gt" => '>',
                        "quot" => '"',
                        "apos" => '\'',
                        autre => return Err(format!("entité inconnue &{};", autre)),
                    },
                };
                if let Some(p) = pile.last_mut() {
                    p.texte.push(c);
                }
            }
            Event::End(_) => {
                let n = pile.pop().ok_or("balise fermante orpheline")?;
                match pile.last_mut() {
                    Some(p) => p.enfants.push(n),
                    None => racine = Some(n),
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if !pile.is_empty() {
        return Err("document tronqué".into());
    }
    racine.ok_or_else(|| "document vide".into())
}

/// Échappe un texte pour un contenu ou un attribut XML.
pub fn esc(s: &str) -> String {
    quick_xml::escape::escape(s).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lit_un_arbre_avec_espace_de_noms_et_entites() {
        let x = r#"<?xml version="1.0"?><h:a xmlns:h="urn:x" v="1"><b>x &amp; y&#233;</b><c k="&lt;"/></h:a>"#;
        let r = lire(x).unwrap();
        assert_eq!(r.nom, "a");
        assert_eq!(r.attribut("v"), Some("1"));
        assert_eq!(r.texte_de("b"), Some("x & yé"));
        assert_eq!(r.enfant("c").unwrap().attribut("k"), Some("<"));
    }

    #[test]
    fn refuse_un_document_tronque() {
        assert!(lire("<a><b>").is_err());
    }
}
