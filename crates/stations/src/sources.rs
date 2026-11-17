//! Parsers of the issuers' pages (H1, H2): titles, validity, prices, member sites, read
//! again at every refresh. Nothing here is typed by hand: a page whose structure changed
//! gives an error (« source cassée ») instead of a guess.
//!
//! Pages: Nordic France (national pass: prices and the member list by association),
//! Espace Nordique Jurassien (FAQ: validity rules; season pass shop: site groups),
//! Haute-Savoie Nordic (NP 74), Nordic Alpes du Sud (prices, shop, one page per domain
//! with day prices and contacts), Nordic Isère (Isère-Drôme pass).

use serde::Serialize;
use skimap_refuges::privacy::{self, Phone};

use crate::titres::{decode_entities, normalise};

/// A price read on a page.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Price {
    /// `saison`, `hebdo`, `semaine`, `journee`, `3h`, `saison_site`, or the label of the
    /// page for site prices.
    pub product: String,
    /// `adulte`, `jeune`, `senior`, or empty when the page does not say.
    pub category: String,
    /// Ages as written.
    pub ages: Option<String>,
    /// Amount, euros.
    pub eur: f64,
    /// `prevente`, `promo_flash`, `promo`, `plein_tarif`, `tarif_unique`.
    pub tier: String,
    /// First day (inclusive), if written.
    pub from: Option<String>,
    /// Last day (inclusive), if written and consistent with the season.
    pub until: Option<String>,
    /// Season as written (`2026-2027`).
    pub season: String,
    /// Source document id.
    pub doc: String,
    /// Line of the page that carries the amount.
    pub quote: String,
}

/// A member site as listed by an issuer.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MemberSpec {
    /// Label as written.
    pub label: String,
    /// Departments where to look for its census domains.
    pub depts: Vec<String>,
    /// Name keys (parts of the label: « Queyras : Abriès – Arvieux » gives three).
    pub keys: Vec<String>,
    /// Group heading of the source (association of the national list, shop section).
    pub group: String,
}

/// A title read on the issuers' pages.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Titre {
    /// Stable id.
    pub id: String,
    /// Name.
    pub name: String,
    /// `national`, `massif`, `departemental`, `regroupement`, `site`.
    pub scope: String,
    /// Issuer.
    pub issuer: String,
    /// Products.
    pub products: Vec<String>,
    /// Validity as quoted.
    pub validity: String,
    /// Source of the validity.
    pub validity_doc: String,
    /// Notes (French), quoted or derived.
    pub notes: Vec<String>,
    /// Members.
    pub members: Vec<MemberSpec>,
    /// Validity outside the census.
    pub abroad: Option<String>,
    /// Prices.
    pub prices: Vec<Price>,
}

/// Day and season prices, and contacts, of one domain page (Nordic Alpes du Sud).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SitePage {
    /// Name on the page.
    pub name: String,
    /// Page.
    pub url: String,
    /// Departments of the shop section, for matching.
    pub depts: Vec<String>,
    /// Season written in the price blocks (one only, else no price is kept).
    pub season: Option<String>,
    /// Prices (`product` is the label of the page: « Journée Adulte », a domain name…).
    pub prices: Vec<Price>,
    /// Phones of the manager and of the tourist office.
    pub phones: Vec<Phone>,
}

/// A site rule quoted from a page.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SiteRule {
    /// Departments.
    pub depts: Vec<String>,
    /// Rule (quoted sentences).
    pub rule: String,
    /// Source document id.
    pub doc: String,
}

/// Lines of the text of an HTML page: block elements end a line; scripts and styles are
/// dropped; entities decoded; whitespace and apostrophes normalised; empty lines dropped.
pub fn html_lines(html: &str) -> Vec<String> {
    const BLOCK: &[&str] = &[
        "p", "br", "div", "li", "tr", "td", "th", "h1", "h2", "h3", "h4", "h5", "h6", "section",
        "ul", "ol", "table", "article", "header", "footer", "nav", "title", "option", "dt", "dd",
    ];
    let lower = html.to_ascii_lowercase();
    let b = html.as_bytes();
    let mut out = String::with_capacity(html.len() / 2);
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'<' {
            let rest = &lower[i..];
            let skip = if rest.starts_with("<script") {
                rest.find("</script>").map(|e| e + 9)
            } else if rest.starts_with("<style") {
                rest.find("</style>").map(|e| e + 8)
            } else if rest.starts_with("<!--") {
                rest.find("-->").map(|e| e + 3)
            } else {
                rest.find('>').map(|e| e + 1)
            };
            let name: String = rest[1..]
                .trim_start_matches('/')
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric())
                .collect();
            out.push(if BLOCK.contains(&name.as_str()) {
                '\n'
            } else {
                ' '
            });
            i += skip.unwrap_or(rest.len());
            continue;
        }
        let next = html[i..].find('<').map_or(html.len(), |e| i + e);
        out.push_str(&html[i..next]);
        i = next;
    }
    decode_entities(&out)
        .lines()
        .map(normalise)
        .filter(|l| !l.is_empty())
        .collect()
}

/// `href` values of an HTML page.
pub fn hrefs(html: &str) -> Vec<String> {
    html.split("href=\"")
        .skip(1)
        .filter_map(|s| s.split('"').next())
        .map(decode_entities)
        .collect()
}

/// Links of an HTML page: `href` and the text of the link (tags removed), in order.
pub fn links(html: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for part in html.split("<a ").skip(1) {
        let Some(h) = part
            .split("href=\"")
            .nth(1)
            .and_then(|x| x.split('"').next())
        else {
            continue;
        };
        let Some(gt) = part.find('>') else { continue };
        let inner = &part[gt + 1..];
        let inner = inner.split("</a>").next().unwrap_or_default();
        let text = html_lines(inner).join(" ");
        out.push((decode_entities(h), text));
    }
    out
}

/// Lower-case ASCII form for keyword tests.
pub fn fold(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'à' | 'â' | 'ä' | 'À' | 'Â' => 'a',
            'é' | 'è' | 'ê' | 'ë' | 'É' | 'È' | 'Ê' => 'e',
            'î' | 'ï' | 'Î' => 'i',
            'ô' | 'ö' | 'Ô' => 'o',
            'ù' | 'û' | 'ü' | 'Û' => 'u',
            'ç' | 'Ç' => 'c',
            '-' | '\'' | '’' | '–' => ' ',
            c => c.to_ascii_lowercase(),
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Departments of the census (INSEE codes, stable) and the regional names the issuers use.
const DEPTS: &[(&str, &[&str])] = &[
    ("alpes de haute provence", &["04"]),
    ("haute savoie", &["74"]),
    ("hautes alpes", &["05"]),
    ("alpes maritimes", &["06"]),
    ("haute loire", &["43"]),
    ("hautes pyrenees", &["65"]),
    ("pyrenees atlantiques", &["64"]),
    ("pyrenees orientales", &["66"]),
    ("haut rhin", &["68"]),
    ("bas rhin", &["67"]),
    ("haute saone", &["70"]),
    ("territoire de belfort", &["90"]),
    ("puy de dome", &["63"]),
    ("ain", &["01"]),
    ("allier", &["03"]),
    ("ardeche", &["07"]),
    ("ardennes", &["08"]),
    ("ariege", &["09"]),
    ("aveyron", &["12"]),
    ("cantal", &["15"]),
    ("correze", &["19"]),
    ("doubs", &["25"]),
    ("drome", &["26"]),
    ("isere", &["38"]),
    ("jura", &["39"]),
    ("loire", &["42"]),
    ("lozere", &["48"]),
    ("nievre", &["58"]),
    ("savoie", &["73"]),
    ("vosges", &["88"]),
    (
        "massif central",
        &["03", "07", "12", "15", "19", "42", "43", "48", "63"],
    ),
    ("jurassien", &["01", "25", "39"]),
    ("alsace", &["67", "68", "88", "90"]),
    ("catalan", &["66"]),
    ("alpes du sud", &["04", "05", "06"]),
];

/// Departments named in a heading (longest names first, no overlap).
pub fn depts_in(heading: &str) -> Vec<String> {
    let mut h = format!(" {} ", fold(heading));
    let mut out: Vec<String> = Vec::new();
    let mut names: Vec<&(&str, &[&str])> = DEPTS.iter().collect();
    names.sort_by_key(|(n, _)| std::cmp::Reverse(n.len()));
    for (n, codes) in names {
        let pat = format!(" {n} ");
        let pat_prefix = format!(" {n}");
        let found = h.contains(&pat) || (n.len() > 6 && h.contains(&pat_prefix));
        if found {
            h = h.replace(&pat_prefix, " ");
            for c in *codes {
                if !out.iter().any(|x| x == c) {
                    out.push((*c).to_owned());
                }
            }
        }
    }
    out
}

/// Amounts in euros of a line (`214 €`, `142€`, `22,5 €`).
pub fn amounts(line: &str) -> Vec<f64> {
    let mut out = Vec::new();
    let chars: Vec<char> = line.chars().collect();
    for (i, c) in chars.iter().enumerate() {
        if *c != '€' {
            continue;
        }
        let mut j = i;
        while j > 0 && chars[j - 1] == ' ' {
            j -= 1;
        }
        let end = j;
        while j > 0 && (chars[j - 1].is_ascii_digit() || chars[j - 1] == ',' || chars[j - 1] == '.')
        {
            j -= 1;
        }
        let num: String = chars[j..end].iter().collect::<String>().replace(',', ".");
        if let Ok(v) = num.trim_matches('.').parse::<f64>() {
            out.push(v);
        }
    }
    out
}

const MONTHS: &[&str] = &[
    "janvier",
    "fevrier",
    "mars",
    "avril",
    "mai",
    "juin",
    "juillet",
    "aout",
    "septembre",
    "octobre",
    "novembre",
    "decembre",
];

/// Calendar year of a month of a season (`2026-2027`): August to December in the first
/// year, January to July in the second.
fn season_year(season: &str, month: u32) -> Option<i32> {
    let y: i32 = season.get(..4)?.parse().ok()?;
    Some(if month >= 8 { y } else { y + 1 })
}

/// French dates of a line (`1er octobre`, `15 novembre`, `09/10`, `15/11/2026`) in the
/// order written, as `YYYY-MM-DD` within `season`.
pub fn dates(line: &str, season: &str) -> Vec<String> {
    let f = fold(line).replace(['(', ')', ',', '='], " ");
    let w: Vec<&str> = f.split_whitespace().collect();
    let mut out = Vec::new();
    for (i, t) in w.iter().enumerate() {
        // dd/mm or dd/mm/yyyy
        let parts: Vec<&str> = t.trim_end_matches('.').split('/').collect();
        if parts.len() >= 2
            && parts
                .iter()
                .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
        {
            let (d, m) = (parts[0].parse::<u32>().ok(), parts[1].parse::<u32>().ok());
            if let (Some(d), Some(m)) = (d, m)
                && (1..=31).contains(&d)
                && (1..=12).contains(&m)
            {
                let y = match parts.get(2).and_then(|y| y.parse::<i32>().ok()) {
                    Some(y) => Some(y),
                    None => season_year(season, m),
                };
                if let Some(y) = y {
                    out.push(format!("{y:04}-{m:02}-{d:02}"));
                }
            }
            continue;
        }
        // « 1er octobre [2026] », « 15 novembre »
        if let Some(m) = MONTHS.iter().position(|m| t == m) {
            let d = i
                .checked_sub(1)
                .and_then(|j| w[j].trim_end_matches("er").parse::<u32>().ok());
            if let Some(d) = d {
                let m = m as u32 + 1;
                let y = w
                    .get(i + 1)
                    .and_then(|y| y.parse::<i32>().ok())
                    .filter(|y| (2000..2100).contains(y))
                    .or_else(|| season_year(season, m));
                if let Some(y) = y {
                    out.push(format!("{y:04}-{m:02}-{d:02}"));
                }
            }
        }
    }
    out
}

/// Day after a `YYYY-MM-DD` date.
pub fn next_day(d: &str) -> Option<String> {
    let date = chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").ok()?;
    date.succ_opt().map(|x| x.format("%Y-%m-%d").to_string())
}

/// Season written in a line (`2026 – 2027`, `2026-2027`, `2025/2026`) as `YYYY-YYYY`.
pub fn season_in(line: &str) -> Option<String> {
    let digits: Vec<(usize, i32)> = line
        .char_indices()
        .filter(|(i, _)| line[*i..].len() >= 4)
        .filter_map(|(i, _)| {
            let s = line.get(i..i + 4)?;
            let before_digit = i > 0 && line.as_bytes()[i - 1].is_ascii_digit();
            let after_digit = line.as_bytes().get(i + 4).is_some_and(u8::is_ascii_digit);
            (s.chars().all(|c| c.is_ascii_digit()) && !before_digit && !after_digit)
                .then(|| s.parse().ok().map(|y| (i, y)))
                .flatten()
        })
        .collect();
    digits
        .windows(2)
        .find(|w| w[1].1 == w[0].1 + 1 && w[1].0 - w[0].0 <= 9)
        .map(|w| format!("{}-{}", w[0].1, w[1].1))
}

/// Name keys of a member label: the label without « Domaine(s) nordique(s) de… »,
/// « Espace nordique », « Station », split on « – », « - », « : », « / », « , ».
pub fn member_keys(label: &str) -> Vec<String> {
    let mut l = label.to_owned();
    for p in [
        "Domaines Nordiques",
        "Domaine Nordique",
        "Domaines nordiques",
        "Domaine nordique",
        "Espace Nordique",
        "Espace nordique",
        "Sites nordiques",
        "Site nordique",
        "Site Nordique",
    ] {
        l = l.replace(p, " ");
    }
    l.replace([':', '/', ',', '|', '(', ')'], "–")
        .replace(" - ", "–")
        .replace(" -", "–")
        .split('–')
        .map(|s| {
            s.trim()
                .trim_start_matches("de la ")
                .trim_start_matches("de l'")
                .trim_start_matches("du ")
                .trim_start_matches("des ")
                .trim_start_matches("de ")
                .trim_start_matches("d'")
                .trim()
                .to_owned()
        })
        .filter(|s| !s.is_empty() && !skimap_osm::census::name_tokens(s).is_empty())
        .collect()
}

fn member(label: &str, depts: &[String], group: &str) -> MemberSpec {
    MemberSpec {
        label: label.to_owned(),
        depts: depts.to_vec(),
        keys: member_keys(label),
        group: group.to_owned(),
    }
}

#[allow(clippy::too_many_arguments)]
fn price(
    product: &str,
    category: &str,
    ages: Option<&str>,
    eur: f64,
    tier: &str,
    span: (Option<String>, Option<String>),
    season: &str,
    doc: &str,
    quote: &str,
) -> Price {
    Price {
        product: product.to_owned(),
        category: category.to_owned(),
        ages: ages.map(str::to_owned),
        eur,
        tier: tier.to_owned(),
        from: span.0,
        until: span.1,
        season: season.to_owned(),
        doc: doc.to_owned(),
        quote: quote.to_owned(),
    }
}

/// Error of a parser: what was not found.
pub type ParseResult<T> = std::result::Result<T, String>;

fn find(lines: &[String], pred: impl Fn(&str) -> bool) -> Option<(usize, &String)> {
    lines.iter().enumerate().find(|(_, l)| pred(&fold(l)))
}

/// Nordic France national pass page: season, prices, validity, notes, members by
/// association.
pub fn parse_nf_national(lines: &[String], doc: &str) -> ParseResult<Titre> {
    let season = lines
        .iter()
        .find(|l| fold(l).contains("tarifs en vigueur"))
        .and_then(|l| season_in(l))
        .ok_or("saison des tarifs introuvable")?;
    let mut prices = Vec::new();
    let mut cat: Option<(&str, Option<String>)> = None;
    let mut list_start = None;
    for (i, l) in lines.iter().enumerate() {
        let f = fold(l);
        if f.starts_with("pass nordique national adulte") {
            cat = Some(("adulte", None));
        } else if f.starts_with("pass nordique national jeune") {
            let ages = l
                .split_once('(')
                .and_then(|(_, r)| r.split_once(')'))
                .map(|(a, _)| a.to_owned());
            cat = Some(("jeune", ages));
        } else if f.starts_with("liste complete des domaines") {
            list_start = Some(i + 1);
            break;
        } else if let Some((c, ages)) = &cat
            && let Some(eur) = amounts(l).first().copied()
        {
            let d = dates(l, &season);
            let (tier, span) = if f.starts_with("promo") || f.contains("prevente") {
                ("prevente", (d.first().cloned(), d.get(1).cloned()))
            } else if f.starts_with("plein tarif") {
                ("plein_tarif", (d.first().cloned(), None))
            } else {
                continue;
            };
            prices.push(price(
                "saison",
                c,
                ages.as_deref(),
                eur,
                tier,
                span,
                &season,
                doc,
                l,
            ));
        }
    }
    if prices.len() < 2 {
        return Err("prix du Pass national introuvables".into());
    }
    let validity = lines
        .iter()
        .find(|l| fold(l).contains("valable sur tous les sites nordiques adherents"))
        .cloned()
        .ok_or("phrase de validité introuvable")?;
    let notes: Vec<String> = lines
        .iter()
        .filter(|l| fold(l).contains("a l exclusion") || fold(l).contains("snowfarming"))
        .cloned()
        .collect();
    let abroad = lines
        .iter()
        .find(|l| fold(l).contains("domaines suisses"))
        .cloned();
    // Member list: association headings end with « : », department sub-headings.
    let start = list_start.ok_or("liste des domaines introuvable")?;
    let mut members = Vec::new();
    let mut depts: Vec<String> = Vec::new();
    let mut group = String::new();
    for l in &lines[start..] {
        let f = fold(l);
        if l.starts_with('#') {
            break;
        }
        if f.starts_with("pour decouvrir") || f.starts_with("http") || f.starts_with("achetez") {
            continue;
        }
        let is_member = [
            "domaine", "domaines", "espace", "station", "centre", "stade", "site",
        ]
        .iter()
        .any(|p| f.starts_with(p));
        if l.trim_end().ends_with(':') {
            depts = depts_in(l);
            group = l.trim_end_matches(':').trim().to_owned();
        } else if !is_member && !depts_in(l).is_empty() && l.split_whitespace().count() <= 5 {
            depts = depts_in(l);
        } else if is_member {
            members.push(member(l, &depts, &group));
        }
    }
    if members.len() < 50 {
        return Err(format!(
            "liste des domaines trop courte ({})",
            members.len()
        ));
    }
    Ok(Titre {
        id: "nordic-pass-national".into(),
        name: "Nordic Pass saison national".into(),
        scope: "national".into(),
        issuer: "Nordic France".into(),
        products: vec!["saison".into()],
        validity,
        validity_doc: doc.into(),
        notes,
        members,
        abroad,
        prices,
    })
}

/// Splits a comma list, keeping commas inside parentheses.
fn split_list(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut cur = String::new();
    for c in s.chars() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            _ => {}
        }
        if c == ',' && depth == 0 {
            out.push(cur.trim().trim_end_matches('.').to_owned());
            cur.clear();
        } else {
            cur.push(c);
        }
    }
    out.push(cur.trim().trim_end_matches('.').to_owned());
    out.retain(|x| !x.is_empty());
    out
}

/// Haute-Savoie Nordic page: Nordic Pass 74 (season prices, validity, members).
pub fn parse_hsn(lines: &[String], doc: &str) -> ParseResult<Titre> {
    let (start, _) = find(lines, |f| f.starts_with("nordic pass haute savoie"))
        .ok_or("bloc « Nordic Pass HAUTE-SAVOIE » introuvable")?;
    let end = lines[start + 1..]
        .iter()
        .position(|l| fold(l).starts_with("nordic pass national"))
        .map_or(lines.len(), |e| start + 1 + e);
    let block = &lines[start..end];
    let season = block
        .iter()
        .find_map(|l| season_in(l))
        .ok_or("saison du NP 74 introuvable")?;
    // « *prix fondus sur les Nordic Pass du 01/10 au 15/11 »
    let window = block
        .iter()
        .find(|l| fold(l).contains("prix fondus"))
        .map(|l| dates(l, &season))
        .unwrap_or_default();
    let mut cat = ("adulte", None::<String>);
    let mut prices = Vec::new();
    for (i, l) in block.iter().enumerate() {
        let f = fold(l);
        if f.starts_with("tarif adulte") {
            cat = ("adulte", None);
        } else if f.starts_with("tarif jeune") {
            let ages = block
                .get(i + 1)
                .filter(|n| fold(n).contains("ans"))
                .cloned();
            cat = ("jeune", ages);
        } else if let Some(eur) = amounts(l).first().copied() {
            if f.starts_with("normal") {
                let from = window.get(1).and_then(|d| next_day(d));
                prices.push(price(
                    "saison",
                    cat.0,
                    cat.1.as_deref(),
                    eur,
                    "plein_tarif",
                    (from, None),
                    &season,
                    doc,
                    l,
                ));
            } else if f.starts_with("prevente") {
                let span = (window.first().cloned(), window.get(1).cloned());
                prices.push(price(
                    "saison",
                    cat.0,
                    cat.1.as_deref(),
                    eur,
                    "prevente",
                    span,
                    &season,
                    doc,
                    l,
                ));
            }
        }
    }
    if prices.len() < 2 {
        return Err("prix du NP 74 introuvables".into());
    }
    let validity = block
        .iter()
        .find(|l| fold(l).contains("permet de skier"))
        .cloned()
        .ok_or("validité du NP 74 introuvable")?;
    let list = block
        .iter()
        .find_map(|l| {
            l.split_once("domaines nordiques suivants :")
                .map(|(_, r)| r.to_owned())
        })
        .ok_or("liste des domaines du NP 74 introuvable")?;
    let depts = vec!["74".to_owned()];
    let members: Vec<MemberSpec> = split_list(&list)
        .iter()
        .map(|m| member(m, &depts, "Haute-Savoie Nordic"))
        .collect();
    let notes = block
        .iter()
        .filter(|l| {
            let f = fold(l);
            f.contains("nocturnes") || f.contains("reduction")
        })
        .cloned()
        .collect();
    Ok(Titre {
        id: "hsn-nordic-pass-74".into(),
        name: "Nordic Pass Haute-Savoie « le 74 »".into(),
        scope: "departemental".into(),
        issuer: "Haute-Savoie Nordic".into(),
        products: vec!["saison".into()],
        validity,
        validity_doc: doc.into(),
        notes,
        members,
        abroad: None,
        prices,
    })
}

/// Domains of the Nordic Alpes du Sud shop: a name followed by « Acheter », under
/// « Billetterie nordique <département> ».
pub fn parse_nads_shop(lines: &[String]) -> Vec<MemberSpec> {
    let mut out = Vec::new();
    let mut depts: Vec<String> = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        let f = fold(l);
        if f.starts_with("billetterie nordique") {
            depts = depts_in(l);
        } else if !depts.is_empty()
            && lines.get(i + 1).is_some_and(|n| fold(n) == "acheter")
            && f != "acheter"
        {
            out.push(member(l, &depts, "Nordic Alpes du Sud"));
        }
    }
    out
}

/// Nordic Alpes du Sud prices page: season and week passes of the Alpes du Sud.
pub fn parse_nads_tarifs(
    lines: &[String],
    doc: &str,
    members: &[MemberSpec],
) -> ParseResult<Vec<Titre>> {
    let season = lines
        .iter()
        .find(|l| fold(l).starts_with("tarifs 20"))
        .and_then(|l| season_in(l))
        .ok_or("saison des tarifs introuvable")?;
    let season_start: i32 = season[..4].parse().map_err(|_| "saison illisible")?;
    let mut saison = Vec::new();
    let mut semaine = Vec::new();
    let mut notes = Vec::new();
    let mut block = "";
    for l in lines {
        let f = fold(l);
        if f.starts_with("nordic pass saison alpes du sud") {
            block = "saison";
            continue;
        } else if f.starts_with("nordic pass semaine alpes du sud") {
            block = "semaine";
            continue;
        } else if f.starts_with("nordic pass") {
            block = "";
            continue;
        }
        let a = amounts(l);
        if block == "saison" && a.len() == 2 {
            let cat = if f.starts_with("adulte") {
                "adulte"
            } else if f.starts_with("jeune") {
                "jeune"
            } else if f.starts_with("senior") {
                "senior"
            } else {
                continue;
            };
            let d = dates(l, &season);
            // A pre-sale end dated before the season is a typo of the page: not used.
            let until = d
                .first()
                .filter(|x| x[..4].parse::<i32>().is_ok_and(|y| y >= season_start))
                .cloned();
            if until.is_none() && !d.is_empty() {
                let n = format!(
                    "date de fin de prévente incohérente dans la source : « {l} » (saison {season})"
                );
                if !notes.contains(&n) {
                    notes.push(n);
                }
            }
            let from = until.as_deref().and_then(next_day);
            saison.push(price(
                "saison",
                cat,
                None,
                a[0],
                "prevente",
                (None, until),
                &season,
                doc,
                l,
            ));
            saison.push(price(
                "saison",
                cat,
                None,
                a[1],
                "plein_tarif",
                (from, None),
                &season,
                doc,
                l,
            ));
        } else if block == "semaine" && a.len() == 2 && f.contains("adulte") && f.contains("jeune")
        {
            semaine.push(price(
                "semaine",
                "adulte",
                None,
                a[0],
                "tarif_unique",
                (None, None),
                &season,
                doc,
                l,
            ));
            semaine.push(price(
                "semaine",
                "jeune",
                None,
                a[1],
                "tarif_unique",
                (None, None),
                &season,
                doc,
                l,
            ));
        }
    }
    if saison.len() < 4 || semaine.len() < 2 {
        return Err("prix Alpes du Sud introuvables".into());
    }
    let mk = |id: &str, name: &str, product: &str, validity: &str, prices: Vec<Price>| Titre {
        id: id.into(),
        name: name.into(),
        scope: "massif".into(),
        issuer: "Nordic Alpes du Sud".into(),
        products: vec![product.into()],
        validity: validity.into(),
        validity_doc: doc.into(),
        notes: {
            let mut n = notes.clone();
            n.push("validité : domaines de la billetterie de Nordic Alpes du Sud (la page des tarifs ne l'écrit pas en toutes lettres)".into());
            n
        },
        members: members.to_vec(),
        abroad: None,
        prices,
    };
    Ok(vec![
        mk(
            "nads-alpes-du-sud-saison",
            "Nordic Pass saison Alpes du Sud",
            "saison",
            "Nordic Pass Saison ALPES DU SUD",
            saison,
        ),
        mk(
            "nads-alpes-du-sud-semaine",
            "Nordic Pass semaine Alpes du Sud",
            "semaine",
            "Nordic Pass semaine ALPES DU SUD",
            semaine,
        ),
    ])
}

/// Nordic Isère page: Isère-Drôme season pass.
pub fn parse_isere(lines: &[String], doc: &str) -> ParseResult<Titre> {
    let (vi, validity) = find(lines, |f| f.contains("valable sur") && f.contains("sites"))
        .ok_or("validité introuvable")?;
    let presale = lines
        .iter()
        .find(|l| fold(l).starts_with("prevente du"))
        .ok_or("dates de prévente introuvables")?;
    // « Prévente du 13 au 31 Octobre 2026 »: the year and month follow the last day.
    let f = fold(presale);
    let w: Vec<&str> = f.split_whitespace().collect();
    let year: i32 = w
        .iter()
        .rev()
        .find_map(|t| t.parse().ok().filter(|y| (2000..2100).contains(y)))
        .ok_or("année de prévente illisible")?;
    let month = w
        .iter()
        .find_map(|t| MONTHS.iter().position(|m| m == t))
        .ok_or("mois de prévente illisible")? as u32
        + 1;
    let days: Vec<u32> = w
        .iter()
        .filter_map(|t| t.trim_end_matches("er").parse().ok())
        .filter(|d| (1..=31).contains(d))
        .collect();
    if days.len() < 2 {
        return Err("jours de prévente illisibles".into());
    }
    let season = if month >= 8 {
        format!("{}-{}", year, year + 1)
    } else {
        format!("{}-{}", year - 1, year)
    };
    let from = format!("{year:04}-{month:02}-{:02}", days[0]);
    let until = format!("{year:04}-{month:02}-{:02}", days[1]);
    let mut members = Vec::new();
    let mut depts: Vec<String> = Vec::new();
    let mut prices = Vec::new();
    for l in &lines[vi + 1..] {
        let f = fold(l);
        if f.starts_with("rechargement") || f.starts_with("prevente") {
            if f.starts_with("rechargement") {
                depts.clear();
            }
        } else if l.chars().all(|c| !c.is_lowercase()) && !depts_in(l).is_empty() {
            depts = depts_in(l);
        } else if !depts.is_empty() {
            members.push(member(l, &depts, "Nordic Isère"));
        }
        let a = amounts(l);
        if a.len() == 2 {
            let cat = if f.starts_with("adulte") {
                "adulte"
            } else if f.starts_with("jeune") {
                "jeune"
            } else if f.starts_with("senior") {
                "senior"
            } else {
                continue;
            };
            let ages = l
                .split_once('(')
                .and_then(|(_, r)| r.split_once(')'))
                .map(|(a, _)| format!("nés {a}"));
            prices.push(price(
                "saison",
                cat,
                ages.as_deref(),
                a[0],
                "prevente",
                (Some(from.clone()), Some(until.clone())),
                &season,
                doc,
                l,
            ));
            prices.push(price(
                "saison",
                cat,
                ages.as_deref(),
                a[1],
                "plein_tarif",
                (next_day(&until), None),
                &season,
                doc,
                l,
            ));
        }
    }
    if members.len() < 5 || prices.len() < 4 {
        return Err("membres ou prix Isère-Drôme introuvables".into());
    }
    Ok(Titre {
        id: "nordic-isere-isere-drome".into(),
        name: "Nordic Pass saison Isère-Drôme".into(),
        scope: "regroupement".into(),
        issuer: "Nordic Isère".into(),
        products: vec!["saison".into()],
        validity: validity.clone(),
        validity_doc: doc.into(),
        notes: vec![format!(
            "{presale} ; le prix entre parenthèses est le plein tarif"
        )],
        members,
        abroad: None,
        prices,
    })
}

/// Rules quoted by the ENJ FAQ.
#[derive(Debug, Clone, PartialEq)]
pub struct EnjFaq {
    /// « Pass saison Montagnes du Jura : … »
    pub saison: String,
    /// « Le Pass hebdo Montagnes du Jura … »
    pub hebdo: String,
    /// « Pass saison de site : … »
    pub site: String,
    /// Day pass rule (séance, réciprocité, pluri-journaliers).
    pub seance: String,
}

/// ENJ FAQ: validity sentences.
pub fn parse_enj_faq(lines: &[String]) -> ParseResult<EnjFaq> {
    let get = |pred: &dyn Fn(&str) -> bool, what: &str| -> ParseResult<String> {
        lines
            .iter()
            .find(|l| pred(&fold(l)))
            .cloned()
            .ok_or(format!("phrase introuvable : {what}"))
    };
    let saison = get(
        &|f| f.starts_with("pass saison montagnes du jura"),
        "Pass saison Montagnes du Jura",
    )?;
    let hebdo = get(
        &|f| f.contains("pass hebdo montagnes du jura") && f.contains("valable"),
        "Pass hebdo",
    )?;
    let site = get(
        &|f| f.starts_with("pass saison de site"),
        "Pass saison de site",
    )?;
    let (i, _) = find(lines, |f| f.contains("pass seance est valable"))
        .ok_or("règle du Pass séance introuvable")?;
    let mut seance = lines[i].clone();
    for l in lines.iter().skip(i + 1).take(2) {
        let f = fold(l);
        if f.contains("reciprocite") || f.contains("pluri journaliers") {
            seance.push(' ');
            seance.push_str(l);
        }
    }
    Ok(EnjFaq {
        saison,
        hebdo,
        site,
        seance,
    })
}

/// A site group of the ENJ season pass shop.
#[derive(Debug, Clone, PartialEq)]
pub struct EnjGroup {
    /// Heading.
    pub label: String,
    /// Departments.
    pub depts: Vec<String>,
    /// Sites of the group (lines between the heading and « Acheter… »).
    pub sites: Vec<String>,
}

/// ENJ season pass shop: groups of sites sold together.
pub fn parse_enj_shop(lines: &[String]) -> Vec<EnjGroup> {
    let mut out = Vec::new();
    let mut depts: Vec<String> = Vec::new();
    let mut cur: Option<EnjGroup> = None;
    for l in lines {
        let f = fold(l);
        if f.starts_with("maj ") {
            continue;
        }
        if f.starts_with("sites nordiques du ") || f.starts_with("sites nordiques de l ain") {
            depts = depts_in(&f.replace("sites nordiques", ""));
            cur = None;
            continue;
        }
        if depts.is_empty() {
            continue;
        }
        if f.starts_with("acheter mon pass") || f.starts_with("ce site ne vend") {
            if let Some(g) = cur.take() {
                out.push(g);
            }
            continue;
        }
        match &mut cur {
            None => {
                cur = Some(EnjGroup {
                    label: l.clone(),
                    depts: depts.clone(),
                    sites: Vec::new(),
                })
            }
            Some(g) => g.sites.push(l.clone()),
        }
    }
    out
}

/// Day and season prices, contacts of a Nordic Alpes du Sud domain page.
pub fn parse_nads_domain(
    lines: &[String],
    url: &str,
    name: Option<String>,
    depts: &[String],
) -> ParseResult<SitePage> {
    let doc = url;
    let status = |l: &str| matches!(fold(l).as_str(), "ferme" | "ouvert");
    // Name: the text of the link to the page (area page), else the line before the
    // opening state of the page.
    let name = match name {
        Some(n) => n,
        None => lines
            .iter()
            .enumerate()
            .find(|(i, l)| !status(l) && lines.get(i + 1).is_some_and(|n| status(n)))
            .map(|(_, l)| l.clone())
            .ok_or("nom du domaine introuvable")?,
    };
    let mut prices = Vec::new();
    let mut seasons: Vec<String> = Vec::new();
    let mut block = "";
    let mut tariff_lines: Vec<(&str, &String)> = Vec::new();
    let mut phones: Vec<Phone> = Vec::new();
    let mut contact: Option<&str> = None;
    for l in lines {
        let f = fold(l);
        match f.as_str() {
            "tarifs journee" => block = "journee",
            "tarifs saison" => block = "saison_site",
            "gestionnaire" => contact = Some("gestionnaire"),
            "office de tourisme" => contact = Some("office_tourisme"),
            _ => {
                if f.starts_with("tarifs au format") || f.starts_with("ski de fond") {
                    block = "";
                    contact = None;
                }
                if !block.is_empty() && (f.starts_with("adultes") || f.starts_with("enfants")) {
                    tariff_lines.push((block, l));
                    if let Some(s) = season_in(&l.replace('/', "-"))
                        && !seasons.contains(&s)
                    {
                        seasons.push(s);
                    }
                }
                if let Some(label) = contact {
                    for p in privacy::find_phones(l) {
                        if !phones.iter().any(|x| x.e164 == p.e164) {
                            phones.push(Phone {
                                display: privacy::display_phone(&p.e164),
                                e164: p.e164,
                                label: label.to_owned(),
                                source: "nordicalpesdusud".to_owned(),
                            });
                        }
                    }
                }
            }
        }
    }
    // One season written in the price blocks applies to the page; none or several: no
    // price is kept (a price without its season is never published).
    let season = (seasons.len() == 1).then(|| seasons[0].clone());
    if let Some(s) = &season {
        for (block, l) in &tariff_lines {
            let category = if fold(l).starts_with("enfants") {
                "jeune"
            } else {
                ""
            };
            // « Journée Adulte:15€ », « <domaine>:75€ », « Massif (5-15ans): 85€ »
            let body = l.split_once(':').map_or(l.as_str(), |(_, r)| r);
            for part in body.split('€') {
                let Some((label, num)) = part.rsplit_once(':') else {
                    continue;
                };
                let num = num.trim().replace(',', ".");
                let Ok(eur) = num.parse::<f64>() else {
                    continue;
                };
                let label = label
                    .replace(s.as_str(), "")
                    .replace(&s.replace('-', "/"), "");
                let label = label.trim().trim_start_matches(':').trim();
                if label.is_empty() {
                    continue;
                }
                prices.push(price(
                    &format!("{block} : {label}"),
                    category,
                    None,
                    eur,
                    "tarif_unique",
                    (None, None),
                    s,
                    doc,
                    l,
                ));
            }
        }
    }
    Ok(SitePage {
        name,
        url: url.to_owned(),
        depts: depts.to_vec(),
        season,
        prices,
        phones,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(s: &str) -> Vec<String> {
        s.lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(str::to_owned)
            .collect()
    }

    #[test]
    fn html_to_lines() {
        let l = html_lines(
            "<html><head><title>T</title><script>x</script></head><body><p>Promo du 1er octobre&nbsp;= <strong>214</strong>&#160;€</p><div>A</div><span>b</span> c</body>",
        );
        assert_eq!(l, vec!["T", "Promo du 1er octobre = 214 €", "A", "b c"]);
    }

    #[test]
    fn small_parsers() {
        assert_eq!(
            amounts("ADULTE : 143 € / Plein : 205 €, 22,5 €"),
            vec![143.0, 205.0, 22.5]
        );
        assert_eq!(
            season_in("Tarifs en vigueur pour la saison 2026 – 2027").as_deref(),
            Some("2026-2027")
        );
        assert_eq!(
            season_in("Adultes : 2025/2026 Journée").as_deref(),
            Some("2025-2026")
        );
        assert_eq!(season_in("de 1951 à 2006"), None);
        assert_eq!(
            dates("Promo du 1er octobre au 15 novembre = 214 €", "2026-2027"),
            vec!["2026-10-01", "2026-11-15"]
        );
        assert_eq!(
            dates("du 01/10 au 15/11", "2026-2027"),
            vec!["2026-10-01", "2026-11-15"]
        );
        assert_eq!(
            dates("(jusqu'au 15/11/2025)", "2026-2027"),
            vec!["2025-11-15"]
        );
        assert_eq!(
            dates("à partir du 16 novembre", "2026-2027"),
            vec!["2026-11-16"]
        );
        assert_eq!(depts_in("Alpes de Haute-Provence"), vec!["04"]);
        assert_eq!(depts_in("Haute-Savoie Nordic :"), vec!["74"]);
        assert_eq!(depts_in("Savoie Nordic :"), vec!["73"]);
        assert_eq!(depts_in("Montagnes Massif Central :").len(), 9);
        assert_eq!(depts_in("Billetterie nordique Hautes Alpes"), vec!["05"]);
        assert_eq!(depts_in("ISERE"), vec!["38"]);
        assert!(depts_in("Domaine Nordique de Cervières -Izoard").is_empty());
        assert_eq!(
            member_keys(
                "Domaines Nordiques du Queyras : Abriès-Ristolas – Arvieux – Ceillac – Molines–Saint-Véran"
            ),
            vec![
                "Queyras",
                "Abriès-Ristolas",
                "Arvieux",
                "Ceillac",
                "Molines",
                "Saint-Véran"
            ]
        );
        assert_eq!(
            member_keys("Domaine Nordique de Cervières -Izoard"),
            vec!["Cervières", "Izoard"]
        );
        assert_eq!(
            split_list("Agy, Vallée de Chamonix (Chamonix, Argentière), Megève."),
            vec!["Agy", "Vallée de Chamonix (Chamonix, Argentière)", "Megève"]
        );
    }

    #[test]
    fn nf_national() {
        let l = lines(
            "Tarifs en vigueur pour la saison 2026 – 2027
            PASS NORDIQUE NATIONAL ADULTE
            Promo du 1er octobre au 15 novembre = 214 €
            Plein tarif à partir du 16 novembre = 255 €
            PASS NORDIQUE NATIONAL JEUNE (5-15 ans révolus)
            Promo du 1er octobre au 15 novembre = 79 €
            Plein tarif à partir du 16 novembre = 94 €
            Le pass national donne l'accès aux pistes de ski de fond des domaines nordiques adhérents à l'exclusion des « nocturnes »
            Nordic France propose un titre national valable sur tous les sites nordiques adhérents (retrouvez la liste)
            Liste complète des domaines accessibles avec le Pass National :
            Les Stations de la Drôme :
            Domaine Nordique de Font d'Urle
            Pour découvrir l'ensemble des domaines :
            https://www.exemple.fr/
            Nordic Alpes du Sud :
            Hautes-Alpes
            Domaine Nordique de Cervières -Izoard",
        );
        let mut l2 = l.clone();
        for i in 0..60 {
            l2.push(format!("Domaine Nordique Fictif {i}"));
        }
        l2.push("#NordicFrance".into());
        l2.push("Domaine Nordique Après".into());
        let t = parse_nf_national(&l2, "nf").unwrap();
        assert_eq!(t.prices.len(), 4);
        assert_eq!(t.prices[0].from.as_deref(), Some("2026-10-01"));
        assert_eq!(t.prices[0].until.as_deref(), Some("2026-11-15"));
        assert_eq!(
            (t.prices[1].tier.as_str(), t.prices[1].eur),
            ("plein_tarif", 255.0)
        );
        assert_eq!(t.prices[2].ages.as_deref(), Some("5-15 ans révolus"));
        assert_eq!(t.members[0].depts, vec!["26"]);
        assert_eq!(t.members[0].group, "Les Stations de la Drôme");
        assert_eq!(t.members[1].depts, vec!["05"]);
        assert_eq!(t.members.len(), 62, "stops at « # »");
        assert_eq!(t.notes.len(), 1);
        // A page without the list is a broken source, not an empty title.
        assert!(parse_nf_national(&l, "nf").is_err());
    }

    #[test]
    fn hsn_nads_isere_enj() {
        let h = lines(
            "Nordic Pass HAUTE-SAVOIE « le 74 »
            Saison 2026-2027
            Tarif adulte
            Normal 178€
            Préventes* 142€
            Tarif jeune
            de 5 à 15 ans révolus
            Normal 52€
            Préventes* 44€
            *prix fondus sur les Nordic Pass du 01/10 au 15/11
            « Le 74 » permet de skier tout l'hiver sur les 25 domaines nordiques de Haute-Savoie
            Valable sur les domaines nordiques suivants : Agy, Vallée de Chamonix (Chamonix, Argentière, Vallorcine).
            Nordic Pass NATIONAL",
        );
        let t = parse_hsn(&h, "hsn").unwrap();
        assert_eq!(t.prices.len(), 4);
        assert_eq!(t.prices[0].from.as_deref(), Some("2026-11-16"));
        assert_eq!(t.prices[1].until.as_deref(), Some("2026-11-15"));
        assert_eq!(t.members.len(), 2);
        assert_eq!(t.prices[2].ages.as_deref(), Some("de 5 à 15 ans révolus"));

        let shop = parse_nads_shop(&lines(
            "Billetterie nordique Hautes Alpes
            Briançonnais
            Cervières Izoard
            Acheter
            Névache
            Acheter",
        ));
        assert_eq!(
            shop.iter().map(|m| m.label.as_str()).collect::<Vec<_>>(),
            vec!["Cervières Izoard", "Névache"]
        );
        let n = parse_nads_tarifs(
            &lines(
                "Tarifs 2026-2027
                Nordic Pass Saison NATIONAL
                ADULTE Prévente (jusqu'au 15/11/2026) : 214 € / ADULTE Plein tarif : 255 €
                Nordic Pass Saison ALPES DU SUD
                ADULTE Prévente (jusqu'au 15/11/2025) : 143 € / ADULTE Plein tarif : 205 €
                JEUNE Prévente (jusqu'au 15/11/2025) : 72 € / JEUNE Plein tarif : 87 €
                Nordic Pass ÉVASION
                Nordic Pass semaine ALPES DU SUD
                ADULTE et SÉNIOR : 77 € / JEUNE : 49 €",
            ),
            "nads",
            &shop,
        )
        .unwrap();
        assert_eq!(n[0].prices.len(), 4);
        assert_eq!(
            n[0].prices[0].until, None,
            "2025 date for the 2026-2027 season"
        );
        assert!(n[0].notes[0].contains("incohérente"));
        assert_eq!(
            n[1].prices.iter().map(|p| p.eur).collect::<Vec<_>>(),
            vec![77.0, 49.0]
        );

        let i = parse_isere(
            &lines(
                "Pass Isère-Drôme
                Ce Nordic Pass saison est valable sur 23 sites de l'Isère et de la Drôm
                ISERE
                Allemont
                Chamrousse
                DROME
                Font d'Urle
                Herbouilly
                Lus la Jarjatte
                Rechargement
                Prévente du 13 au 31 Octobre 2026
                Adulte (de 1951 à 2006) 160€ (175€)
                Jeune (de 2007 à 2022) 57€ (62€)",
            ),
            "isere",
        )
        .unwrap();
        assert_eq!(i.members.len(), 5);
        assert_eq!(i.members[2].depts, vec!["26"]);
        assert_eq!(i.prices[0].from.as_deref(), Some("2026-10-13"));
        assert_eq!(i.prices[1].from.as_deref(), Some("2026-11-01"));
        assert_eq!(i.prices[0].season, "2026-2027");

        let faq = parse_enj_faq(&lines(
            "Pass saison Montagnes du Jura : accès à tous les sites nordiques de l'Ain, du Doubs et du Jura.
            Pass saison de site : accès uniquement sur le site d'achat
            Le Pass séance est valable uniquement sur le site d'achat du Pass.
            Une réciprocité existe quand plusieurs domaines peuvent être reliés ski aux pieds.
            Cette règle est la même pour les Pass pluri journaliers (de 2 à 5 jours).
            Le Pass hebdo Montagnes du Jura (ski et balade) est valable sur TOUS les sites nordiques du massif du Jura.",
        ))
        .unwrap();
        assert!(faq.seance.contains("réciprocité") && faq.seance.contains("pluri"));
        let g = parse_enj_shop(&lines(
            "Sites nordiques du Doubs
            MAJ 09.11 : Appli
            Sites nordiques de la communauté de communes des Lacs et Montagnes du Ht Doubs
            Les Fourgs - la Seigne
            Chapelle des Bois - le Pré Poncet
            Acheter mon Pass à ce site nordique
            Site Nordique d'Arc sous Cicon
            Acheter mon Pass saison à ce site nordique
            Sites nordiques du Jura
            Site nordique de Saint Laurent en Grandvaux
            Ce site ne vend pas de Pass saison pour le moment",
        ));
        assert_eq!(g.len(), 3);
        assert_eq!(g[0].sites.len(), 2);
        assert_eq!(g[0].depts, vec!["25"]);
        assert_eq!(g[2].depts, vec!["39"]);
    }

    #[test]
    fn link_texts() {
        let l = links(
            r#"<a href="/domaines/ancelle"><img src="x"></a> <a class="b" href="/domaines/ancelle"><span>Ancelle</span></a>"#,
        );
        assert_eq!(
            l,
            vec![
                ("/domaines/ancelle".into(), String::new()),
                ("/domaines/ancelle".into(), "Ancelle".into())
            ]
        );
    }

    #[test]
    fn nads_domain() {
        let l = lines(
            "Cervières - Izoard
            FERME
            Cervières - Izoard
            GESTIONNAIRE
            Foyer de ski de fond - Mairie
            04 00 00 00 01 (foyer-hiver) 04.00.00.00.02 (Mairie)
            foyer@exemple.fr
            OFFICE DE TOURISME
            06 00 00 00 03
            Ski de fond
            TARIFS JOURNEE
            Adultes : 2025/2026 Journée Adulte:15€ Journée Jeune: 8€ 3H Adulte : 11€ €
            Enfants : 8€ (5-15ans) -5 ans et +80 ans Gratuit €
            TARIFS SAISON
            Adultes : Massif: 200€ Cervières:75€ €
            TARIFS AU FORMAT PDF",
        );
        let p = parse_nads_domain(&l, "u", None, &["05".into()]).unwrap();
        assert_eq!(p.name, "Cervières - Izoard");
        assert_eq!(p.season.as_deref(), Some("2025-2026"));
        let got: Vec<(String, f64)> = p
            .prices
            .iter()
            .map(|x| (x.product.clone(), x.eur))
            .collect();
        assert_eq!(
            got,
            vec![
                ("journee : Journée Adulte".to_owned(), 15.0),
                ("journee : Journée Jeune".to_owned(), 8.0),
                ("journee : 3H Adulte".to_owned(), 11.0),
                ("saison_site : Massif".to_owned(), 200.0),
                ("saison_site : Cervières".to_owned(), 75.0),
            ]
        );
        assert_eq!(p.phones.len(), 3);
        assert_eq!(p.phones[2].label, "office_tourisme");
    }
}
