//! Rapport HTML autonome, imprimable en PDF depuis n'importe quel navigateur.

use crate::langue::{self, Langue};
use crate::rapport::{Niveau, Rapport, Verdict};
use crate::xml::esc;

const CSS: &str = r#"
:root{--encre:#16181d;--doux:#5b616e;--trait:#d9dce3;--fond:#fff;--ok:#0b7a3e;--ok-fond:#e3f4ea;--alerte:#9a5b00;--alerte-fond:#fdf1dc;--ko:#b3261e;--ko-fond:#fbe4e2}
*{box-sizing:border-box}body{margin:0;background:var(--fond);color:var(--encre);font:14px/1.45 -apple-system,"Segoe UI",Roboto,Helvetica,Arial,sans-serif}
main{max-width:1100px;margin:0 auto;padding:28px 24px 48px}
h1{font-size:22px;margin:0 0 4px}h2{font-size:15px;text-transform:uppercase;letter-spacing:.06em;color:var(--doux);margin:28px 0 8px;border-bottom:1px solid var(--trait);padding-bottom:4px}
.sous{color:var(--doux)}
.verdict{margin:18px 0;padding:16px 18px;border-radius:8px;font-weight:700;font-size:20px;letter-spacing:.02em}
.v-ok{background:var(--ok-fond);color:var(--ok);border:2px solid var(--ok)}.v-alerte{background:var(--alerte-fond);color:var(--alerte);border:2px solid var(--alerte)}.v-ko{background:var(--ko-fond);color:var(--ko);border:2px solid var(--ko)}
.grille{display:grid;grid-template-columns:repeat(auto-fit,minmax(170px,1fr));gap:10px}
.case{border:1px solid var(--trait);border-radius:6px;padding:8px 10px}.case b{display:block;font-size:17px}.case span{color:var(--doux);font-size:12px}
table{width:100%;border-collapse:collapse;font-size:12.5px}th,td{text-align:left;padding:5px 6px;border-bottom:1px solid var(--trait);vertical-align:top}th{color:var(--doux);font-weight:600}
td.n{text-align:right;white-space:nowrap}code,.mono{font-family:ui-monospace,Menlo,Consolas,monospace;font-size:11.5px;word-break:break-all}
.ok{color:var(--ok)}.alerte{color:var(--alerte)}.ko{color:var(--ko);font-weight:600}
ul{margin:6px 0;padding-left:20px}li{margin:3px 0}
.pied{margin-top:36px;color:var(--doux);font-size:12px;border-top:1px solid var(--trait);padding-top:10px}
@media print{main{padding:0}h2{break-after:avoid}tr{break-inside:avoid}.verdict{-webkit-print-color-adjust:exact;print-color-adjust:exact}}
"#;

fn classe_verdict(v: Verdict) -> &'static str {
    match v {
        Verdict::BonAFormater => "v-ok",
        Verdict::ConformeNePasFormater => "v-alerte",
        _ => "v-ko",
    }
}

fn symbole(n: Niveau) -> (&'static str, &'static str) {
    match n {
        Niveau::Support => ("✓", "ok"),
        Niveau::CachePossible => ("≈", "alerte"),
        Niveau::Taille => ("~", "alerte"),
        Niveau::Echec => ("✗", "ko"),
    }
}

pub fn rendre(r: &Rapport, l: Langue) -> String {
    let noms = r.noms_destinations();
    let mut h = String::with_capacity(64 * 1024 + r.fichiers.len() * 400);
    let t = |fr: &'static str, en: &'static str| l.t(fr, en);
    let date = |d: &chrono::DateTime<chrono::Utc>| {
        d.with_timezone(&chrono::Local)
            .format("%d/%m/%Y %H:%M:%S")
            .to_string()
    };

    h.push_str(&format!(
        "<!doctype html><html lang=\"{}\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>{} — {}</title><style>{CSS}</style></head><body><main>",
        if l == Langue::Fr { "fr" } else { "en" },
        esc(&r.nom),
        t("Rapport de déchargement", "Offload report")
    ));
    h.push_str(&format!(
        "<h1>{} « {} »</h1><div class=\"sous\">{} · {} {} · {}</div>",
        t("Déchargement de la carte", "Offload of card"),
        esc(&r.nom),
        date(&r.debut),
        esc(&r.outil),
        esc(&r.version),
        esc(&r.machine)
    ));
    h.push_str(&format!(
        "<div class=\"verdict {}\">{}</div>",
        classe_verdict(r.verdict),
        r.verdict.titre(l)
    ));

    if !r.remarques.is_empty() {
        h.push_str("<ul>");
        for x in &r.remarques {
            h.push_str(&format!("<li>{}</li>", esc(&x.texte(l, &noms))));
        }
        h.push_str("</ul>");
    }

    h.push_str("<div class=\"grille\">");
    for (valeur, libelle) in [
        (
            langue::octets(r.octets, l),
            t("copiés depuis la carte", "copied from the card"),
        ),
        (r.nb_fichiers.to_string(), t("fichiers", "files")),
        (r.nb_clips.to_string(), t("clips", "clips")),
        (
            langue::duree(r.duree_s(), l),
            t("durée totale", "total time"),
        ),
        (
            r.algos
                .iter()
                .map(|a| a.nom())
                .collect::<Vec<_>>()
                .join(" + "),
            t("empreintes", "checksums"),
        ),
        (
            match r.mode {
                crate::rapport::ModeVerif::Complet => t("complète", "full").to_string(),
                crate::rapport::ModeVerif::CompletCarte => {
                    t("complète + relecture carte", "full + card re-read").to_string()
                }
                crate::rapport::ModeVerif::Taille => t("taille seule", "size only").to_string(),
            },
            t("vérification", "verification"),
        ),
    ] {
        h.push_str(&format!(
            "<div class=\"case\"><b>{}</b><span>{}</span></div>",
            esc(&valeur),
            libelle
        ));
    }
    h.push_str("</div>");

    // Source
    h.push_str(&format!("<h2>{}</h2><table>", t("Carte", "Card")));
    let vs = r.volume_source.as_ref();
    for (k, v) in [
        (t("Chemin", "Path"), r.source.clone()),
        (
            t("Volume", "Volume"),
            vs.map(|v| format!("{} ({})", v.nom, v.systeme_fichiers))
                .unwrap_or_default(),
        ),
        (
            t("Lecture moyenne", "Average read"),
            langue::debit(r.profil_source.debit_moyen, l),
        ),
        (t("Opérateur", "Operator"), r.operateur.clone()),
        (t("Projet", "Project"), r.projet.clone()),
        (t("Début", "Start"), date(&r.debut)),
        (t("Fin", "End"), date(&r.fin)),
    ] {
        if !v.is_empty() {
            h.push_str(&format!("<tr><th>{k}</th><td>{}</td></tr>", esc(&v)));
        }
    }
    h.push_str("</table>");

    // Destinations
    h.push_str(&format!(
        "<h2>{}</h2><table><tr><th>#</th><th>{}</th><th>{}</th><th>{}</th><th class=n>{}</th><th class=n>{}</th></tr>",
        t("Destinations", "Destinations"),
        t("Disque et dossier", "Disk and folder"),
        t("Résultat", "Result"),
        t("Manifestes", "Manifests"),
        t("Écriture", "Write"),
        t("Fichiers", "Files"),
    ));
    for (i, d) in r.destinations.iter().enumerate() {
        let niveau = r.niveau_destination(i);
        let (s, c) = symbole(niveau);
        let vol = d
            .volume
            .as_ref()
            .map(|v| format!("{} · {}", v.nom, v.systeme_fichiers))
            .unwrap_or_default();
        let mut resultat = format!("<span class=\"{c}\">{s} {}</span>", niveau.libelle(l));
        if let Some(e) = &d.erreur_fatale {
            resultat.push_str(&format!(
                "<br><span class=ko>{}</span><br>{}",
                esc(&e.message(l)),
                esc(e.action(l))
            ));
        }
        h.push_str(&format!(
            "<tr><td>{}</td><td><b>{}</b><br><span class=mono>{}</span></td><td>{resultat}</td><td class=mono>{}</td><td class=n>{}</td><td class=n>{} / {}</td></tr>",
            i + 1,
            esc(&vol),
            esc(&d.dossier),
            esc(&d.manifestes.join("\n")).replace('\n', "<br>"),
            langue::debit(d.profil.debit_moyen, l),
            d.fichiers_ok,
            r.nb_fichiers
        ));
    }
    h.push_str("</table>");

    if let Some(diag) = &r.diagnostic {
        h.push_str(&format!("<h2>{}</h2><ul>", t("Débit", "Throughput")));
        for f in &diag.faits {
            h.push_str(&format!("<li>{}</li>", esc(f)));
        }
        h.push_str("</ul>");
        if !diag.hypotheses.is_empty() {
            h.push_str("<ul>");
            for x in &diag.hypotheses {
                h.push_str(&format!(
                    "<li><b>{}</b> — {}. <i>{}</i></li>",
                    esc(&x.titre),
                    esc(&x.parce_que),
                    esc(&x.verifier)
                ));
            }
            h.push_str("</ul>");
        }
    }

    // Fichiers en échec d'abord, puis tous les fichiers.
    let echecs: Vec<_> = r
        .fichiers
        .iter()
        .filter(|f| f.erreur_source.is_some() || f.copies.iter().any(|c| c.niveau == Niveau::Echec))
        .collect();
    if !echecs.is_empty() || !r.illisibles.is_empty() {
        h.push_str(&format!(
            "<h2 class=ko>{}</h2><table><tr><th>{}</th><th>{}</th></tr>",
            t("Problèmes", "Problems"),
            t("Fichier", "File"),
            t("Détail et action", "Detail and action")
        ));
        for e in &r.illisibles {
            h.push_str(&format!(
                "<tr><td class=mono>—</td><td class=ko>{}<br>{}</td></tr>",
                esc(&e.message(l)),
                esc(e.action(l))
            ));
        }
        for f in echecs {
            let mut detail = String::new();
            if let Some(e) = &f.erreur_source {
                detail.push_str(&format!(
                    "<span class=ko>{}</span><br>{}<br>",
                    esc(&e.message(l)),
                    esc(e.action(l))
                ));
            } else {
                for c in f.copies.iter().filter(|c| c.niveau == Niveau::Echec) {
                    if let Some(e) = &c.erreur {
                        detail.push_str(&format!(
                            "<b>{}</b> : <span class=ko>{}</span><br>{}<br>",
                            esc(noms.get(c.destination).map(String::as_str).unwrap_or("?")),
                            esc(&e.message(l)),
                            esc(e.action(l))
                        ));
                    }
                }
            }
            h.push_str(&format!(
                "<tr><td class=mono>{}</td><td>{detail}</td></tr>",
                esc(&f.chemin)
            ));
        }
        h.push_str("</table>");
    }

    h.push_str(&format!(
        "<h2>{} ({})</h2><table><tr><th>{}</th><th class=n>{}</th><th>{}</th>",
        t("Fichiers", "Files"),
        r.nb_fichiers,
        t("Chemin", "Path"),
        t("Taille", "Size"),
        r.algo_comparaison.nom()
    ));
    for (i, _) in r.destinations.iter().enumerate() {
        h.push_str(&format!("<th>{}</th>", i + 1));
    }
    h.push_str("</tr>");
    for f in &r.fichiers {
        h.push_str(&format!(
            "<tr><td class=mono>{}</td><td class=n>{}</td><td class=mono>{}</td>",
            esc(&f.chemin),
            langue::octets(f.taille, l),
            esc(f.empreinte(r.algo_comparaison).unwrap_or("—"))
        ));
        for i in 0..r.destinations.len() {
            let n = f.copie(i).map(|c| c.niveau).unwrap_or(Niveau::Echec);
            let (s, c) = symbole(n);
            h.push_str(&format!(
                "<td class=\"{c}\" title=\"{}\">{s}</td>",
                n.libelle(l)
            ));
        }
        h.push_str("</tr>");
    }
    h.push_str("</table>");
    h.push_str(&format!(
        "<p class=sous>✓ {} · ≈ {} · ~ {} · ✗ {}</p>",
        Niveau::Support.libelle(l),
        Niveau::CachePossible.libelle(l),
        Niveau::Taille.libelle(l),
        Niveau::Echec.libelle(l)
    ));

    if !r.exclus.is_empty() || !r.liens.is_empty() {
        h.push_str(&format!(
            "<h2>{}</h2><p class=sous>{}</p><ul>",
            t("Non copiés", "Not copied"),
            t(
                "Fichiers déposés par le système d'exploitation, et liens symboliques.",
                "Files written by the operating system, and symbolic links."
            )
        ));
        for x in r.exclus.iter().chain(r.liens.iter()) {
            h.push_str(&format!("<li class=mono>{}</li>", esc(x)));
        }
        h.push_str("</ul>");
    }

    h.push_str(&format!(
        "<div class=pied>{}</div></main></body></html>",
        t(
            "Outil fourni gratuitement par SuperLoc, en l'état, comme aide à la vérification. Conservez la carte intacte tant que deux copies vérifiées n'existent pas sur deux disques différents. Dans les limites permises par la loi ; vos droits de consommateur ne sont pas affectés.",
            "Tool provided free of charge by SuperLoc, as is, as a verification aid. Keep the card untouched until two verified copies exist on two different disks. To the extent permitted by law; your consumer rights are not affected."
        )
    ));
    h
}
