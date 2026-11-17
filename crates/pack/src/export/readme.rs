//! French how-to written in the export folder: `LISEZMOI.md` (index) and `aide/*.md`
//! (one file per section, each under 100 lines).

use std::fmt::Write as _;

use crate::export::ExportedFile;
use crate::manifest::{Layer, Manifest};

fn size(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1} Mo", n as f64 / 1e6).replace('.', ",")
    } else {
        format!("{} Ko", n.div_ceil(1000))
    }
}

/// The `LISEZMOI.md` of an export.
pub fn lisezmoi(
    m: &Manifest,
    files: &[ExportedFile],
    skipped: &[(String, String)],
    overlays: &[(Layer, String, u8, u8)],
) -> String {
    let site = &m.site.name;
    let total: u64 = files.iter().map(|f| f.bytes).sum();
    let mut s = String::new();
    let _ = writeln!(s, "# skimap — {site} : couches pour Locus Map et OsmAnd\n");
    let _ = writeln!(
        s,
        "Pack {} du {} (format {} v{}). Total : {}.\n",
        m.pack_version,
        m.generated_at.get(..10).unwrap_or(&m.generated_at),
        m.format,
        m.format_version,
        size(total)
    );
    s.push_str(
        "**Prudence.** Ces couches ne disent jamais qu'un endroit est sûr. Les zones d'avalanche sont des avalanches \
         connues ; une avalanche peut se produire ailleurs : lisez le BERA. La neige est une estimation, pas une mesure. \
         Les zones sans réseau viennent des cartes théoriques des opérateurs.\n\n",
    );
    let _ = writeln!(s, "Validité : {}\n", m.valid_for.note);

    s.push_str("## Contenu\n\n| Fichier | Contenu | Éléments | Taille |\n|---|---|---|---|\n");
    for f in files {
        let what = describe(&f.path);
        let count = if f.count == 0 {
            String::new()
        } else {
            f.count.to_string()
        };
        let unit = if f.path.ends_with(".mbtiles") || f.path.ends_with(".sqlitedb") {
            " tuiles"
        } else {
            ""
        };
        let _ = writeln!(
            s,
            "| `{}` | {what} | {count}{unit} | {} |",
            f.path,
            size(f.bytes)
        );
    }
    s.push('\n');
    if !skipped.is_empty() {
        s.push_str("Non exporté :\n\n");
        for (id, why) in skipped {
            let _ = writeln!(s, "- `{id}` : {why}");
        }
        s.push('\n');
    }
    s.push_str(
        "Couleurs : pistes vertes, bleues, rouges, noires selon la difficulté OSM (un tronçon partagé prend la plus \
         facile ; la plus difficile est dans la description) ; zones protégées en vert (Biodiv'sports en orange, \
         règles dans la description) ; zones sans réseau en violet ; tronçons en zone d'avalanche connue en rouge épais.\n\n",
    );
    if !overlays.is_empty() {
        s.push_str("Surcouches (tuiles PNG 256 px, transparentes hors classe) :\n\n");
        for (l, stem, minz, maxz) in overlays {
            let entries = l
                .raster
                .as_ref()
                .map(|r| &r.legend)
                .or(l.classes.as_ref().map(|c| &c.legend));
            let legend: Vec<String> = entries
                .map(|r| {
                    r.iter()
                        .map(|e| {
                            if e.color.ends_with("00") {
                                format!("{} : transparent", e.label)
                            } else {
                                format!("{} : {}", e.label, &e.color[..7])
                            }
                        })
                        .collect()
                })
                .unwrap_or_default();
            let zooms = match &l.raster {
                Some(r) => format!(
                    "zooms {minz} à {maxz} (zoom {} calculé, au-delà agrandi)",
                    r.maxzoom
                ),
                None => format!(
                    "zooms {minz} à {maxz}, dessinées à chaque zoom depuis les polygones vectoriels du pack"
                ),
            };
            let _ = writeln!(
                s,
                "- `{stem}` : {} ; {zooms} ; {}.",
                l.title,
                legend.join(", ")
            );
        }
        s.push('\n');
    }

    s.push_str(&format!(
        "## Copier sur le téléphone\n\n\
         Téléphone branché, débogage USB autorisé :\n\n\
         ```sh\n\
         adb push data/out/{id}/export /sdcard/Download/skimap-{id}\n\
         ```\n\n\
         ou copier le dossier par câble (MTP) dans `Téléchargements`.\n\n",
        id = m.site.id
    ));

    s.push_str(
        "## Locus Map 4\n\n\
         Traces et points (KML conseillé : Locus garde les couleurs et les dossiers) :\n\n\
         1. Ouvrir l'application Fichiers, aller dans `Téléchargements/skimap-…/kml/`, toucher `pistes.kml` et choisir « Locus Map ».\n\
         2. Dans l'écran d'import, choisir ou créer un dossier (par exemple « skimap ») et valider.\n\
         3. Recommencer pour les autres `.kml` (refuges, zones, sans réseau, avalanches).\n\
         4. Autre voie : menu ☰ > Points / Traces > Importer, puis choisir le fichier.\n\n\
         Surcouches (pentes, neige) :\n\n\
         1. Copier les `.mbtiles` de `locus/` dans le dossier des cartes de Locus : `Locus/maps/` (Android 11 et plus : \
         `Android/data/menion.android.locus/files/Locus/maps/`), ou toucher le fichier dans Fichiers et l'ouvrir avec Locus Map.\n\
         2. Les cartes apparaissent dans Cartes > Hors ligne > Personnelles.\n\
         3. Garder le fond de carte habituel et ajouter la pente en surcouche : Cartes > (menu ⋮) > Surcouche de carte, \
         choisir `skimap Pentes…`, régler la transparence. (Le chemin exact du menu dépend de la version : le noter dans la liste ci-dessous.)\n\n",
    );
    s.push_str(
        "## OsmAnd\n\n\
         Traces et points (GPX conseillé ; OsmAnd lit aussi le KML en le convertissant) :\n\n\
         1. Dans Fichiers, toucher un `.gpx` de `gpx/` et choisir « OsmAnd » ; ou dans OsmAnd : Menu > Mes lieux > Traces > Importer.\n\
         2. Pour les pistes, importer les fichiers par couleur (`pistes-vertes.gpx`, `pistes-bleues.gpx`…) : chaque fichier \
         porte sa couleur.\n\
         3. Afficher : Configurer la carte > Traces > cocher les fichiers.\n\
         4. Les refuges sont des points (favoris de la trace) : ils s'affichent avec la trace.\n\n\
         Surcouches :\n\n\
         1. Activer l'extension « Cartes en ligne » (Menu > Extensions), nécessaire aux surcouches raster.\n\
         2. Toucher un `.sqlitedb` de `osmand/` dans Fichiers et l'ouvrir avec OsmAnd (il est copié dans le dossier `tiles` d'OsmAnd), \
         ou le copier dans `Android/media/net.osmand.plus/files/tiles/`.\n\
         3. Configurer la carte > Surcouche > choisir `pentes` (ou `neige-…`), régler la transparence avec le curseur.\n\
         4. OsmAnd affiche une surcouche jusqu'à deux zooms au-dessus de son zoom maximal : les tuiles vont jusqu'au zoom 16 (pentes) \
         et 15 (neige).\n\n",
    );
    s.push_str(
        "## Liste de vérification (demain, sur le Pixel)\n\n\
         Noter pour chaque ligne : OK / problème, et une capture d'écran.\n\n\
         Locus Map :\n\n\
         - [ ] `kml/pistes.kml` importé : les pistes ont leur couleur (vert, bleu, rouge, noir) et leur nom ; la description \
         (toucher une piste) donne difficulté, damage, longueur.\n\
         - [ ] `kml/refuges-refuges-info.kml` : points visibles, description avec le dernier commentaire daté.\n\
         - [ ] `kml/zones-protegees.kml` : polygones remplis, règles lisibles dans la description.\n\
         - [ ] `kml/sans-reseau.kml` : polygones violets ; temps d'import et fluidité de la carte (beaucoup de polygones).\n\
         - [ ] `kml/avalanches-troncons.kml` (sites couverts par la CLPA) : tronçons rouges épais, longueur et texte d'alerte.\n\
         - [ ] `kml/itineraires.kml` : itinéraires de preuve en bleu (sur pistes) et orange (hors piste), description avec longueur et temps.\n\
         - [ ] `locus/pentes.mbtiles` : visible en carte hors ligne, puis en surcouche transparente sur le fond habituel ; \
         alignement correct avec le relief (comparer à un ravin connu) ; zoom 17-18 encore affiché.\n\
         - [ ] `locus/neige-*.mbtiles` : visibles en surcouche.\n\
         - [ ] Chemin exact des menus d'import et de surcouche, à recopier ici.\n\n\
         OsmAnd :\n\n\
         - [ ] `gpx/pistes-*.gpx` importés : couleur par fichier, nom des pistes en touchant une trace.\n\
         - [ ] `gpx/refuges-*.gpx` : points visibles avec description.\n\
         - [ ] `gpx/zones-protegees.gpx`, `gpx/sans-reseau.gpx` : contours visibles (OsmAnd trace les polygones comme des lignes fermées).\n\
         - [ ] `gpx/itineraires.gpx` : itinéraires visibles, extrémités en points.\n\
         - [ ] Un `.kml` importé dans OsmAnd (conversion) : noter le résultat.\n\
         - [ ] `osmand/pentes.sqlitedb` en surcouche : visible, transparence réglable, alignée, zooms 10 à 18.\n\
         - [ ] `osmand/neige-*.sqlitedb` en surcouche.\n\
         - [ ] Mode avion : tout reste affiché (rien ne vient du réseau).\n\n\
         Tailles : noter la place prise dans chaque application (Paramètres Android > Applications > stockage) avant et après import.\n\n",
    );
    s.push_str("## Sources et licences\n\n");
    for a in &m.attributions {
        let _ = writeln!(s, "- {a}");
    }
    s.push_str(
        "\nUsage personnel. Refuges : trois fichiers séparés, un par source (CC BY-SA refuges.info et Camptocamp, ODbL OSM), \
         tant que la compatibilité des licences (ticket L3) n'est pas tranchée.\n",
    );
    s
}

fn describe(path: &str) -> &'static str {
    let stem = path.rsplit('/').next().unwrap_or(path);
    let stem = stem.split('.').next().unwrap_or(stem);
    match stem {
        "pistes" => "pistes, un dossier par couleur",
        "pistes-vertes" => "pistes vertes",
        "pistes-bleues" => "pistes bleues",
        "pistes-rouges" => "pistes rouges",
        "pistes-noires" => "pistes noires",
        "pistes-autres" => "pistes sans difficulté connue",
        "refuges-refuges-info" => "refuges et abris, refuges.info",
        "refuges-osm" => "refuges et abris, OpenStreetMap",
        "refuges-camptocamp" => "refuges et abris, Camptocamp",
        "zones-protegees" => "zones protégées, règles en description",
        "sans-reseau" => "zones sans aucun réseau (112 compris)",
        "avalanches-troncons" => "tronçons de pistes en zone d'avalanche connue",
        "avalanches-emprises" => "emprises des avalanches historiques (CLPA)",
        "itineraires" => "itinéraires de preuve du routage (sur pistes, hors piste)",
        "pentes" => "pentes ≥ 30° (LiDAR HD 5 m)",
        "neige-hauteur" => "hauteur de neige estimée",
        "neige-piste-damee" => "neige pour piste damée (classes)",
        "neige-hors-piste" => "neige pour hors piste (classes)",
        "LISEZMOI" => "ce mode d'emploi",
        _ => "",
    }
}

fn slug(title: &str) -> &'static str {
    let t = title.to_lowercase();
    if t.starts_with("contenu") {
        "contenu"
    } else if t.starts_with("copier") {
        "copier"
    } else if t.starts_with("locus") {
        "locus"
    } else if t.starts_with("osmand") {
        "osmand"
    } else if t.starts_with("liste") {
        "verification"
    } else {
        "sources"
    }
}

/// Splits the how-to at its `## ` sections: `LISEZMOI.md` keeps the introduction and a
/// table of contents, each section goes to `aide/<section>.md`.
pub fn split_parts(text: &str) -> Vec<(String, String)> {
    let mut parts: Vec<(String, String)> = Vec::new();
    let mut intro = String::new();
    let mut current: Option<(String, String)> = None;
    for line in text.lines() {
        if let Some(title) = line.strip_prefix("## ") {
            if let Some(c) = current.take() {
                parts.push(c);
            }
            current = Some((
                title.to_owned(),
                format!("# {title}\n\n[← Sommaire](../LISEZMOI.md)\n"),
            ));
        } else if let Some((_, body)) = current.as_mut() {
            body.push_str(line);
            body.push('\n');
        } else {
            intro.push_str(line);
            intro.push('\n');
        }
    }
    if let Some(c) = current.take() {
        parts.push(c);
    }
    intro.push_str("## Sommaire\n\n");
    for (title, _) in &parts {
        let _ = writeln!(intro, "- [{title}](aide/{}.md)", slug(title));
    }
    let mut out = vec![("LISEZMOI.md".to_owned(), intro)];
    out.extend(
        parts
            .into_iter()
            .map(|(title, body)| (format!("aide/{}.md", slug(&title)), body)),
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_sections_with_links() {
        let parts = split_parts("# T\n\nintro\n\n## Contenu\n\nx\n\n## Locus Map 4\n\ny\n");
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[0].0, "LISEZMOI.md");
        assert!(parts[0].1.contains("- [Contenu](aide/contenu.md)"));
        assert!(parts[0].1.contains("- [Locus Map 4](aide/locus.md)"));
        assert_eq!(parts[2].0, "aide/locus.md");
        assert!(
            parts[2]
                .1
                .starts_with("# Locus Map 4\n\n[← Sommaire](../LISEZMOI.md)\n")
        );
        assert!(parts[2].1.contains("\ny\n"));
    }
}
