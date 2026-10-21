//! Langue des messages : français ou anglais.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Langue {
    #[default]
    Fr,
    En,
}

impl Langue {
    pub fn depuis(code: &str) -> Langue {
        if code.to_ascii_lowercase().starts_with("en") {
            Langue::En
        } else {
            Langue::Fr
        }
    }

    /// Choisit entre le texte français et le texte anglais.
    pub fn t<'a>(self, fr: &'a str, en: &'a str) -> &'a str {
        match self {
            Langue::Fr => fr,
            Langue::En => en,
        }
    }
}

/// Taille lisible, en unités décimales comme les fabricants de disques.
pub fn octets(n: u64, l: Langue) -> String {
    let (ko, mo, go, to) = match l {
        Langue::Fr => ("ko", "Mo", "Go", "To"),
        Langue::En => ("kB", "MB", "GB", "TB"),
    };
    let n = n as f64;
    let v = if n >= 1e12 {
        format!("{:.2} {to}", n / 1e12)
    } else if n >= 1e9 {
        format!("{:.2} {go}", n / 1e9)
    } else if n >= 1e6 {
        format!("{:.1} {mo}", n / 1e6)
    } else if n >= 1e3 {
        format!("{:.0} {ko}", n / 1e3)
    } else {
        return match l {
            Langue::Fr => format!("{n} octets"),
            Langue::En => format!("{n} bytes"),
        };
    };
    match l {
        Langue::Fr => v.replace('.', ","),
        Langue::En => v,
    }
}

/// Débit lisible.
pub fn debit(octets_par_s: f64, l: Langue) -> String {
    if octets_par_s <= 0.0 {
        return "—".to_string();
    }
    let mo = octets_par_s / 1e6;
    let (mos, gos) = match l {
        Langue::Fr => ("Mo/s", "Go/s"),
        Langue::En => ("MB/s", "GB/s"),
    };
    let v = if mo >= 1000.0 {
        format!("{:.2} {gos}", mo / 1000.0)
    } else if mo >= 100.0 {
        format!("{mo:.0} {mos}")
    } else {
        format!("{mo:.1} {mos}")
    };
    match l {
        Langue::Fr => v.replace('.', ","),
        Langue::En => v,
    }
}

/// Durée lisible.
pub fn duree(secondes: f64, l: Langue) -> String {
    let s = secondes.max(0.0).round() as u64;
    let (h, m, s) = (s / 3600, (s % 3600) / 60, s % 60);
    match (h, m, l) {
        (0, 0, _) => format!("{s} s"),
        (0, _, _) => format!("{m} min {s:02} s"),
        (_, _, _) => format!("{h} h {m:02} min"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tailles_lisibles() {
        assert_eq!(octets(512, Langue::Fr), "512 octets");
        assert_eq!(octets(1_500_000_000, Langue::Fr), "1,50 Go");
        assert_eq!(octets(1_500_000_000, Langue::En), "1.50 GB");
    }

    #[test]
    fn durees_lisibles() {
        assert_eq!(duree(42.0, Langue::Fr), "42 s");
        assert_eq!(duree(125.0, Langue::Fr), "2 min 05 s");
        assert_eq!(duree(3725.0, Langue::Fr), "1 h 02 min");
    }
}
