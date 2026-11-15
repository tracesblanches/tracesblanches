//! Catalogue of the nordic access titles (« redevances », Nordic Pass), read by hand on
//! the issuers' pages on 15/11/2026 (H1, H2: #132, #133).
//!
//! Every fact carries its source document and a quote; [`crate::titres::check_sources`]
//! re-reads each source (with `personal_sources`) and flags a fact whose quote is gone
//! (HTML) or whose document changed (PDF, SHA-256).
//!
//! Reuse: none of these pages displays a reuse licence (associations: Nordic France, ENJ,
//! Haute-Savoie Nordic, Nordic Alpes du Sud, Nordic Isère). Prices are facts and the
//! extraction is small, but a systematic extraction of a member list could fall under the
//! database right (Code de la propriété intellectuelle, art. L341-1, L342-1): the whole
//! catalogue is therefore `personal_use_only` until the issuers agree (letters L1 and L4).

/// A source document.
#[derive(Debug, Clone, Copy)]
pub struct Doc {
    /// Id used by facts.
    pub id: &'static str,
    /// Publisher.
    pub publisher: &'static str,
    /// Title.
    pub title: &'static str,
    /// URL.
    pub url: &'static str,
    /// Downloader source name.
    pub source: &'static str,
    /// `html` (quotes checked in the text) or `pdf` (SHA-256 checked).
    pub format: &'static str,
    /// SHA-256 of the PDF read on 15/11/2026.
    pub sha256: Option<&'static str>,
    /// Date read.
    pub read_on: &'static str,
}

/// A price.
#[derive(Debug, Clone, Copy)]
pub struct Price {
    /// `saison`, `hebdo` (7 days), `semaine`, `journee`, `3h`.
    pub product: &'static str,
    /// `adulte`, `jeune`, `senior`.
    pub category: &'static str,
    /// Ages as written by the issuer.
    pub ages: Option<&'static str>,
    /// Amount, euros.
    pub eur: f64,
    /// `prevente`, `promo_flash`, `promo`, `plein_tarif`, `tarif_unique`.
    pub tier: &'static str,
    /// First day of the tier (inclusive), if dated.
    pub from: Option<&'static str>,
    /// Last day of the tier (inclusive), if dated.
    pub until: Option<&'static str>,
    /// Season as written (`2026-2027`): a price without season is never published.
    pub season: &'static str,
    /// Source document id.
    pub doc: &'static str,
    /// Text of the source that carries the amount (checked for HTML sources).
    pub quote: &'static str,
}

/// A member site of a title, as listed by the issuer, with the census domains it covers.
#[derive(Debug, Clone, Copy)]
pub struct Member {
    /// Label as written in the source.
    pub label: &'static str,
    /// Departments where to look for its domains.
    pub depts: &'static [&'static str],
    /// Name keys for automatic matching (all words of a key must be in a domain name,
    /// OpenSkiMap name or commune).
    pub keys: &'static [&'static str],
    /// Census domains checked by hand (replace the automatic matching when not empty).
    pub domains: &'static [&'static str],
    /// `verifie` (hand-checked: same name, commune or piste name), `partiel` (the census
    /// domain also holds trails of non-member sites), `probable` (hand-checked on the map,
    /// not named by the source), `auto` (automatic matching).
    pub confidence: &'static str,
}

/// A title.
#[derive(Debug, Clone, Copy)]
pub struct Titre {
    /// Stable id.
    pub id: &'static str,
    /// Name.
    pub name: &'static str,
    /// `national`, `massif`, `departemental`, `regroupement`, `site`.
    pub scope: &'static str,
    /// Issuer.
    pub issuer: &'static str,
    /// Products of the title (season, week...).
    pub products: &'static [&'static str],
    /// Validity as quoted from the source.
    pub validity: &'static str,
    /// Source of the validity.
    pub validity_doc: &'static str,
    /// Restrictions and notes (French).
    pub notes: &'static [&'static str],
    /// Member sites.
    pub members: &'static [Member],
    /// Validity outside the census (Switzerland...).
    pub abroad: Option<&'static str>,
    /// Prices.
    pub prices: &'static [Price],
}

/// Rules of a site title (day pass) by issuer area, quoted.
#[derive(Debug, Clone, Copy)]
pub struct SiteRule {
    /// Departments where the rule applies.
    pub depts: &'static [&'static str],
    /// Rule.
    pub rule: &'static str,
    /// Source.
    pub doc: &'static str,
}

/// Day prices of one site.
#[derive(Debug, Clone, Copy)]
pub struct SitePrices {
    /// Census domains.
    pub domains: &'static [&'static str],
    /// Site name in the source.
    pub label: &'static str,
    /// Prices.
    pub prices: &'static [Price],
}

/// Read date of every document.
pub const READ_ON: &str = "2026-11-15";

/// Source documents.
pub const DOCS: &[Doc] = &[
    Doc {
        id: "nf-pass-national",
        publisher: "Nordic France",
        title: "Pass National (tarifs 2026-2027 et liste des domaines accessibles)",
        url: "https://www.nordicfrance.fr/qui-sommes-nous/pass-national",
        source: "nordicfrance",
        format: "html",
        sha256: None,
        read_on: READ_ON,
    },
    Doc {
        id: "enj-grille",
        publisher: "Espace Nordique Jurassien",
        title: "Grille tarifaire Pass Nordique saison 2026/2027 (PDF)",
        url: "https://www.espacenordiquejurassien.com/csx/scripts/downloader2.php?filename=T004/fichier/fa/2e/FR.fw6mt6wepbn&mime=application%2Fpdf&originalname=GRILLE_TARIFAIRE_2026-2027.pdf",
        source: "enj",
        format: "pdf",
        sha256: Some("46d4b3f1710cf9ca41e8d8fb5e68902713354ab692e0043f98d9d6aee00a0afb"),
        read_on: READ_ON,
    },
    Doc {
        id: "enj-faq",
        publisher: "Espace Nordique Jurassien",
        title: "FAQ de la vente en ligne des Pass",
        url: "https://pass.espacenordiquejurassien.com/faq",
        source: "enj",
        format: "html",
        sha256: None,
        read_on: READ_ON,
    },
    Doc {
        id: "enj-pass-saison",
        publisher: "Espace Nordique Jurassien",
        title: "Vente des Pass Saison : sites nordiques du Doubs, du Jura et de l'Ain",
        url: "https://pass.espacenordiquejurassien.com/home-pass-saison",
        source: "enj",
        format: "html",
        sha256: None,
        read_on: READ_ON,
    },
    Doc {
        id: "hsn-np74",
        publisher: "Haute-Savoie Nordic",
        title: "Les différents Nordic Pass et les tarifs",
        url: "https://www.haute-savoie-nordic.com/forfait-nordic-pass-74/",
        source: "haute-savoie-nordic",
        format: "html",
        sha256: None,
        read_on: READ_ON,
    },
    Doc {
        id: "nads-tarifs",
        publisher: "Nordic Alpes du Sud",
        title: "Nordic Pass - Tarifs 2026-2027",
        url: "https://www.nordicalpesdusud.com/nordic-pass/tarifs-2026-2027",
        source: "nordicalpesdusud",
        format: "html",
        sha256: None,
        read_on: READ_ON,
    },
    Doc {
        id: "nads-grille",
        publisher: "Nordic Alpes du Sud",
        title: "Tarifs des redevances Nordic Alpes du Sud saison 2026-2027 (PDF)",
        url: "https://www.nordicalpesdusud.com/system/resources/W1siZiIsIjIwMjYvMDkvMTAvMTMvNTYvMDMvZWNlY2NiYTMtYjQxYi00Mzc3LWE4YTctNDcwOTliMDlhMGMxL0dyaWxsZSBUYXJpZmFpcmUgTm9yZGljIEFscGVzIGR1IFN1ZCBcdTAwMjYgRG9tYWluZXMgMjAyNi0yMDI3LnBkZiJdXQ/Grille%20Tarifaire%20Nordic%20Alpes%20du%20Sud%20%26%20Domaines%202026-2027.pdf",
        source: "nordicalpesdusud",
        format: "pdf",
        sha256: Some("df623278fde72dd8cd065311a0bd413ba78e7da3d448eac36f3b1b2a62ef0dc1"),
        read_on: READ_ON,
    },
    Doc {
        id: "nads-billetterie",
        publisher: "Nordic Alpes du Sud",
        title: "Billetterie nordique (domaines où acheter un Nordic Pass)",
        url: "https://billetterie-nordic-nads.intence.tech/",
        source: "nads-billetterie",
        format: "html",
        sha256: None,
        read_on: READ_ON,
    },
    Doc {
        id: "nads-cervieres",
        publisher: "Nordic Alpes du Sud",
        title: "Cervières - Izoard (gestionnaire, tarifs journée et saison)",
        url: "https://www.nordicalpesdusud.com/domaines/cervieres",
        source: "nordicalpesdusud",
        format: "html",
        sha256: None,
        read_on: READ_ON,
    },
    Doc {
        id: "isere-pass",
        publisher: "Nordic Isère",
        title: "Pass Isère-Drôme",
        url: "https://www.nordic-isere.fr/pass-isere/",
        source: "nordic-isere",
        format: "html",
        sha256: None,
        read_on: READ_ON,
    },
];

const S: &str = "2026-2027";

/// One row of a price table: the arguments are the columns of [`Price`].
#[allow(clippy::too_many_arguments)]
const fn p(
    product: &'static str,
    category: &'static str,
    ages: Option<&'static str>,
    eur: f64,
    tier: &'static str,
    from: Option<&'static str>,
    until: Option<&'static str>,
    doc: &'static str,
    quote: &'static str,
) -> Price {
    Price {
        product,
        category,
        ages,
        eur,
        tier,
        from,
        until,
        season: S,
        doc,
        quote,
    }
}

const fn m(
    label: &'static str,
    depts: &'static [&'static str],
    keys: &'static [&'static str],
) -> Member {
    Member {
        label,
        depts,
        keys,
        domains: &[],
        confidence: "auto",
    }
}

const fn v(
    label: &'static str,
    depts: &'static [&'static str],
    domains: &'static [&'static str],
    confidence: &'static str,
) -> Member {
    Member {
        label,
        depts,
        keys: &[],
        domains,
        confidence,
    }
}

const D26: &[&str] = &["26"];
const D38: &[&str] = &["38"];
const D73: &[&str] = &["73"];
const D74: &[&str] = &["74"];
const D04: &[&str] = &["04"];
const D05: &[&str] = &["05"];
const D06: &[&str] = &["06"];
const DMC: &[&str] = &["03", "07", "12", "15", "42", "43", "48", "63"];
const D01: &[&str] = &["01"];
const D25: &[&str] = &["25"];
const D39: &[&str] = &["39"];
const D68: &[&str] = &["68"];
const DBA: &[&str] = &["68", "88", "90"];
const D88: &[&str] = &["88"];
const D66: &[&str] = &["66"];
const D09: &[&str] = &["09"];

/// Members of the Espace Nordique Jurassien (Ain, Doubs, Jura), checked by hand against
/// the census on 15/11/2026: list of the national pass page (Nordic France) and of the
/// season pass shop (ENJ, `enj-pass-saison`).
pub const ENJ_MEMBERS: &[Member] = &[
    v(
        "Domaine Nordique d’Apremont",
        D01,
        &["site-9231568"],
        "verifie",
    ),
    v(
        "Domaine Nordique de Belleydoux",
        D01,
        &["site-13704619"],
        "verifie",
    ),
    v(
        "Centre Montagnard de Lachat",
        D01,
        &["site-9227579"],
        "verifie",
    ),
    v(
        "Domaine Nordique de Giron",
        D01,
        &["site-13704653", "net-334848400"],
        "verifie",
    ),
    v(
        "Domaine Nordique d’Hauteville – La Praille",
        D01,
        &["site-13664042"],
        "verifie",
    ),
    v(
        "Domaine Nordique du Plateau de Retord – Cuvéry -La Chapelle",
        D01,
        &["site-9227575"],
        "verifie",
    ),
    // ENJ shop: « La Vattay - Mijoux - la Valserine - Menthières ».
    v(
        "Station Monts Jura",
        D01,
        &["site-3161151", "net-1021944915"],
        "verifie",
    ),
    v(
        "Domaine Nordique de Sur Lyand Grand Colombier",
        D01,
        &["site-9227542"],
        "verifie",
    ),
    v(
        "Domaines Nordiques du Canton de Montbenoît : Arc-sous-Cicon – Chaux de Gilley – Gilley – Haut Saugeais Blanc",
        D25,
        &["net-123797916", "net-97559764"],
        "verifie",
    ),
    // Combe Saint-Pierre (Charquemont): the census network starts at Fournet-Blancheroche.
    v(
        "Domaine Nordique de la Combe Saint-Pierre",
        D25,
        &["net-206107197"],
        "probable",
    ),
    // Plateau de Maîche: network of Damprichard; Plateau du Russey: Le Bélieu.
    v(
        "Domaine Nordique du Plateau de Maiche – Combe St Pierre – Plateau de Russey",
        D25,
        &["net-914473998", "net-925024307"],
        "probable",
    ),
    // ENJ shop: « Le Gounefay - le Larmont, La Malmaison, Les Grandes Dessus ».
    v(
        "Domaine Nordique du Grand Pontarlier",
        D25,
        &["site-13767199", "site-3533848"],
        "verifie",
    ),
    // ENJ shop: « Les Fourgs - la Seigne, Métabief - le Mont d'Or, Mouthe - Chez Liadet,
    // Chapelle des Bois - le Pré Poncet »; Chaux-Neuve: 1 km between Mouthe and
    // Chapelle-des-Bois.
    v(
        "Domaine Nordique des Lacs et Montagnes du Haut-Doubs",
        D25,
        &[
            "site-2764548",
            "site-3161203",
            "site-2783351",
            "site-13681086",
            "net-920680329",
        ],
        "verifie",
    ),
    // ENJ shop: « Les Crêtes Franco Suisses / le Gardot / le Meix Musy / Les Combes ».
    v(
        "Domaine Nordique du Val de Morteau",
        D25,
        &["net-159224166"],
        "verifie",
    ),
    // ENJ shop: « Site nordique du Val de Vennes (Fournets Luisans) ».
    v(
        "Domaine Nordique du Val de Vennes",
        D25,
        &["net-185284964"],
        "verifie",
    ),
    v(
        "Domaine Nordique de Haute Joux",
        D39,
        &["site-2783292"],
        "verifie",
    ),
    v("Domaine Nordique de Frasne", D39, &[], "verifie"),
    // ENJ shop: « Village - les Lacs - Risoux ».
    v(
        "Domaine Nordique de Bellefontaine",
        D39,
        &["site-13714785", "site-3161183"],
        "verifie",
    ),
    v(
        "Domaine Nordique de Foncine-le-Haut",
        D39,
        &["site-13768889"],
        "verifie",
    ),
    // ENJ shop: « Hautes Combes du Jura : Lajoux - Les Moussières - La Pesse ».
    v(
        "Domaine Nordique des Hautes Combes du Jura",
        D39,
        &["site-13699336", "site-13705091", "site-13705382"],
        "verifie",
    ),
    v(
        "Domaine Nordique du Lac des Rouges-Truites",
        D39,
        &["site-13768933"],
        "verifie",
    ),
    v(
        "Domaine Nordique des Crozets – Jura Sud",
        D39,
        &["site-13134088"],
        "verifie",
    ),
    v("Domaine Nordique de Longchaumois", D39, &[], "verifie"),
    v(
        "Domaine Nordique de Morbier",
        D39,
        &["site-13714784"],
        "verifie",
    ),
    // Saint-Pierre (between Prénovel and Saint-Laurent) carries the trails of Nanchez.
    v(
        "Domaine Nordique de Nanchez – Prénovel – Chaux-des-Prés-Trémontagne",
        D39,
        &["site-13134081", "site-13134084"],
        "probable",
    ),
    v(
        "Domaine Nordique de Saint-Laurent-en-Grandvaux",
        D39,
        &["site-12168453"],
        "verifie",
    ),
    v(
        "Domaine Nordique Haut Jura Saint Claude",
        D39,
        &[],
        "verifie",
    ),
    v(
        "Station des Rousses",
        D39,
        &["site-3161169", "net-478721195"],
        "verifie",
    ),
];

/// Domains of the Jura massif (01, 25, 39) that are not ENJ members on 15/11/2026: no
/// Jura pass there (checked by hand: absent from both lists).
pub const JURA_NOT_MEMBERS: &[&str] = &[
    "net-541063042", // Brénod
    "net-166390325", // Bolandoz
    "net-185244921", // Les Premiers Sapins
    "net-185244936", // Les Premiers Sapins
    "net-190381222", // Le Crouzet
    "net-274102261", // Arçon
    "net-88197525",  // Le Laveron
    "site-15411343", // Malbuisson - La Fuvelle
    "net-421543053", // Grande-Rivière Château
];

/// Members of Haute-Savoie Nordic listed for the NP 74 (`hsn-np74`).
pub const HSN_MEMBERS: &[Member] = &[
    // Agy = Saint-Sigismond (research report 08, hand-checked).
    v("Agy", D74, &["net-196901574"], "verifie"),
    m("Bellevaux les Mouilles", D74, &["bellevaux"]),
    m("Haut-Giffre", D74, &["giffre", "samoens", "sixt"]),
    m("La Chapelle d’Abondance", D74, &["chapelle abondance"]),
    m("La Clusaz – Les Confins", D74, &["clusaz"]),
    m("Le Grand-Bornand", D74, &["bornand"]),
    m("Le Salève", D74, &["saleve"]),
    m("Le Semnoz", D74, &["semnoz"]),
    m("Les Contamines Montjoie", D74, &["contamines"]),
    m("Les Moises Habère Poche", D74, &["moises"]),
    m("Megève", D74, &["megeve"]),
    m("Morzine Avoriaz", D74, &["morzine"]),
    // Orange = La Roche-sur-Foron (research report 08).
    v("Orange", D74, &["net-1012797281"], "verifie"),
    m("Plaine Joux les Brasses", D74, &["brasses"]),
    m("Plateau de Beauregard", D74, &["beauregard"]),
    m("Plateau de Brison Solaison", D74, &["solaison"]),
    m("Plateau des Glières", D74, &["glieres"]),
    m("Praz de Lys Sommand", D74, &["praz lys"]),
    v(
        "Sur Lyand Grand Colombier",
        D01,
        &["site-9227542"],
        "verifie",
    ),
    m(
        "Vallée de Chamonix (Chamonix, Argentière, Vallorcine)",
        D74,
        &["chamonix", "vallorcine", "chosalets"],
    ),
    m(
        "Domaine Nordique de La Chapelle-Rambaud",
        D74,
        &["chapelle rambaud"],
    ),
];

/// Members of Nordic Isère and of the Drôme stations (`isere-pass`).
pub const ISERE_DROME_MEMBERS: &[Member] = &[
    m("Allemont", D38, &["allemont"]),
    // Alpe du Grand Serre = La Morte (research report 08).
    v("Alpe du Grand Serre", D38, &["net-1133481879"], "verifie"),
    m("Autrans-Méaudre", D38, &["autrans", "meaudre"]),
    m("Beldina (Prapoutel - Les 7 Laux)", D38, &["beldina"]),
    m("Chamrousse", D38, &["chamrousse"]),
    m("Chichilianne", D38, &["chichilianne"]),
    m("Col de Marcieu", D38, &["marcieu"]),
    m(
        "Chamechaude (Le Sappey en Chartreuse, le Col de Porte)",
        D38,
        &["sappey", "porte"],
    ),
    m("Col d’Ornon", D38, &["ornon"]),
    m("Corrençon en Vercors", D38, &["correncon"]),
    m("Gresse-en-vercors", D38, &["gresse"]),
    m(
        "La Ruchère (Saint Christophe sur Guiers)",
        D38,
        &["ruchere"],
    ),
    m("Lans en Vercors", D38, &["lans"]),
    m("Le Barioz", D38, &["barioz"]),
    // Coulmes = Rencurel (research report 08).
    v(
        "Les Coulmes (Presles, Rencurel)",
        D38,
        &["net-78675377"],
        "verifie",
    ),
    m("Les Signaraux (La Motte d’Aveillans)", D38, &["aveillans"]),
    m(
        "Saint Hugues (Saint Pierre de Chartreuse)",
        D38,
        &["pierre chartreuse"],
    ),
    // The census domain « Espace Nordique du Haut vercors » (Corrençon) carries Villard.
    v("Villard de Lans", D38, &["site-2798243"], "probable"),
    m("Font d’Urle", D26, &["urle"]),
    m("Herbouilly", D26, &["herbouilly", "agnan"]),
    m("Stade Raphaël Poirée", D26, &["rousset"]),
    m("le Grand Echaillon", D26, &["echaillon"]),
    m("Lus la Jarjatte", D26, &["jarjatte"]),
];

/// Members of Nordic Alpes du Sud (04, 05, 06) from the national pass list.
pub const NADS_MEMBERS: &[Member] = &[
    m(
        "Domaine Nordique de La Colle Saint-Michel",
        D04,
        &["colle michel"],
    ),
    m("Domaine Nordique de Ratéry", D04, &["ratery"]),
    m(
        "Domaines Nordiques de l’Ubaye – Larche – Saint-Paul-sur-Ubaye – Barcelonnette",
        D04,
        &["larche", "paul ubaye", "barcelonnette"],
    ),
    m("Domaine Nordique d’Ancelle", D05, &["ancelle"]),
    m(
        "Domaines Nordiques du Champsaur-Valgaudemar – La Chapelle-en-Valgaudemar",
        D05,
        &["valgaudemar"],
    ),
    m("Domaine Nordique de Crévoux", D05, &["crevoux"]),
    v(
        "Domaine Nordique de Cervières -Izoard",
        D05,
        &["net-152532950"],
        "verifie",
    ),
    m("Domaine Nordique du Dévoluy", D05, &["devoluy"]),
    m("Domaine Nordique de Freissinières", D05, &["freissinieres"]),
    // Census « Crots » carries « Boucle Le sentier de la Draye ».
    v(
        "Domaine Nordique de La Draye",
        D05,
        &["net-66410529"],
        "verifie",
    ),
    m(
        "Domaine Nordique Villar-d’Arène -Pays de la Meije",
        D05,
        &["villar arene"],
    ),
    m("Domaine Nordique de Névache", D05, &["nevache"]),
    m("Domaine Nordique en Vallouise", D05, &["vallouise"]),
    m(
        "Domaine Nordique de Puy-Saint-Vincent",
        D05,
        &["puy vincent"],
    ),
    m(
        "Domaines Nordiques du Queyras : Abriès-Ristolas – Arvieux – Ceillac – Molines–Saint-Véran",
        D05,
        &["abries", "arvieux", "ceillac", "molines", "veran"],
    ),
    m("Domaine Nordique de Réallon", D05, &["reallon"]),
    m(
        "Domaine Nordique de Serre-Chevalier",
        D05,
        &["serre chevalier"],
    ),
    m("Station Gap Bayard", D05, &["bayard"]),
    m(
        "Domaine Nordique de Val des Prés-Les Alberts",
        D05,
        &["val pres"],
    ),
    m(
        "Domaine Nordique de Villard-Saint-Pancrace",
        D05,
        &["villard pancrace"],
    ),
    m(
        "Domaine Nordique de Beuil-Valberg",
        D06,
        &["valberg", "beuil"],
    ),
    m("Domaine Nordique de Gréolières", D06, &["greolieres"]),
    m("Domaine Nordique de Castérino", D06, &["casterino"]),
    m("Domaine Nordique du Boréon", D06, &["boreon"]),
];

/// Domains sold by the Nordic Alpes du Sud shop (billetterie-nordic-nads.intence.tech,
/// read 15/11/2026): the national list without Castérino, plus Le Fanget (Vallée de la
/// Blanche), absent from the national list.
pub const NADS_SHOP_MEMBERS: &[Member] = &[
    m("Colmars Ratery", D04, &["ratery"]),
    m("La Colle Saint Michel", D04, &["colle michel"]),
    m(
        "Nordic Ubaye",
        D04,
        &["larche", "paul ubaye", "barcelonnette"],
    ),
    m("Le Fanget", D04, &["fanget"]),
    v("Cervières Izoard", D05, &["net-152532950"], "verifie"),
    m("Névache", D05, &["nevache"]),
    m("Serre Chevalier", D05, &["serre chevalier"]),
    m("Val-des-Près Les Alberts", D05, &["val pres"]),
    m("Villard-Saint-Pancrace", D05, &["villard pancrace"]),
    m("Villar d'Arène Pays de la Meije", D05, &["villar arene"]),
    m("Ancelle", D05, &["ancelle"]),
    m("Le Dévoluy", D05, &["devoluy"]),
    m("Champsaur Valgaudemar", D05, &["valgaudemar"]),
    m("Gap Bayard", D05, &["bayard"]),
    m("Freissinières", D05, &["freissinieres"]),
    m("Puy-Saint-Vincent", D05, &["puy vincent"]),
    m("Nordic en Vallouise", D05, &["vallouise"]),
    m(
        "Espace Nordique du Queyras",
        D05,
        &["abries", "arvieux", "ceillac", "molines", "veran"],
    ),
    m("Crévoux", D05, &["crevoux"]),
    v("La Draye", D05, &["net-66410529"], "verifie"),
    m("Réallon", D05, &["reallon"]),
    m("Le Boréon Saint Martin Vésubie", D06, &["boreon"]),
    m("Gréolières-les-neiges", D06, &["greolieres"]),
    m("Beuil | Valberg", D06, &["valberg", "beuil"]),
];

/// Members of the other associations of the national pass list.
pub const OTHER_NF_MEMBERS: &[Member] = &[
    // Savoie Nordic.
    m("Domaine Nordique d’Aillons-Margériaz", D73, &["aillon"]),
    m("Domaine Nordique d’Arêches-Beaufort", D73, &["beaufort"]),
    m(
        "Domaine Nordique d’Aussois Val Cenis Sardières",
        D73,
        &["sardieres"],
    ),
    m("Domaine Nordique de Val Cenis Bramans", D73, &["ambin"]),
    m("Domaine Nordique de Bessans", D73, &["bessans"]),
    m(
        "Domaine Nordique Vallon de Champagny le Haut",
        D73,
        &["champagny"],
    ),
    m(
        "Domaine Nordique des Entremonts en Chartreuse",
        D73,
        &["entremonts"],
    ),
    m("Espace Nordique du Grand Coin", D73, &["grand coin"]),
    m("Espace Nordique Les Karellis", D73, &["karellis"]),
    m("Domaine Nordique Méribel Lac de Tuéda", D73, &["meribel"]),
    m("Domaine Nordique Méribel Altiport", D73, &["meribel"]),
    m("Domaine Nordique de Nâves", D73, &["naves"]),
    m(
        "Domaine Nordique de Pralognan-la-Vanoise",
        D73,
        &["pralognan"],
    ),
    m("Domaine Nordique des Saisies", D73, &["saisies", "diamant"]),
    m("Domaine Nordique de Savoie Grand Revard", D73, &["revard"]),
    m(
        "Domaine Nordique de Valloire Galibier",
        D73,
        &["galibier", "valloire"],
    ),
    // Montagnes Massif Central.
    m(
        "Domaine Nordique de la Baraque des Bouviers",
        DMC,
        &["bouviers"],
    ),
    m("Domaine Nordique de Bonnecombe", DMC, &["bonnecombe"]),
    m("Domaine Nordique de Brameloup", DMC, &["brameloup"]),
    m(
        "Domaine Nordique du Col de la Loge – Domaine Nordique du Haut-Forez",
        DMC,
        &["loge", "chalmazel"],
    ),
    m(
        "Domaine Nordique Bleymard Mont Lozère",
        DMC,
        &["bleymard", "lozere"],
    ),
    m(
        "Domaine Nordique des Crêtes du Forez",
        DMC,
        &["cretes forez"],
    ),
    m("Domaine Nordique de Laguiole", DMC, &["laguiole"]),
    m(
        "Domaine Nordique de Laubert – Plateau du Roy",
        DMC,
        &["laubert"],
    ),
    m("Domaine Nordique du Mas de la Barque", DMC, &["barque"]),
    m("Domaine Nordique du Meygal", DMC, &["meygal"]),
    m("Domaine Nordique du Mézenc", DMC, &["mezenc", "estables"]),
    m(
        "Domaine Nordique de la Montagne Ardéchoise",
        DMC,
        &["chavade"],
    ),
    m("Domaine Nordique de Montoncel", DMC, &["montoncel"]),
    m("Espace Nordique des Monts du Pilat", DMC, &["pilat"]),
    m(
        "Domaine Nordique de Nasbinals – Fer à Cheval",
        DMC,
        &["nasbinals"],
    ),
    m(
        "Espace Nordique de Pailherols en Carladès",
        DMC,
        &["pailherols"],
    ),
    m("Domaine Nordique de Parrot", DMC, &["parrot"]),
    m(
        "Domaine Nordique de Prat de Bouc – Haute Planèze",
        DMC,
        &["prat bouc", "lioran"],
    ),
    m("Domaine Nordique de Saint-Urcize", DMC, &["urcize"]),
    // ADPSF Haut-Rhin, SMIBA, Vosges.
    m("Domaine Nordique du Lac Blanc", D68, &["lac blanc"]),
    m("Domaine Nordique du Markstein", D68, &["markstein"]),
    m("Domaine Nordique des Trois Fours", D68, &["trois fours"]),
    m(
        "Domaine Nordique du Ballon d’Alsace",
        DBA,
        &["ballon alsace"],
    ),
    // The census network « Gérardmer » carries « Liaison La Bresse / Xonrupt / Gérardmer »:
    // La Bresse-Lispach is part of it, Gérardmer and Xonrupt are not listed by Nordic France.
    v(
        "Domaine Nordique de La Bresse-Lispach",
        D88,
        &["net-22890557"],
        "partiel",
    ),
    // Pyrénées Catalanes Nordiques, Ariège Pyrénées Nordiques.
    m(
        "Domaine Nordique du Capcir – Col de la Llose – Forêt de la Matte – La Quillane",
        D66,
        &["llose", "capcir", "matte", "quillane"],
    ),
    m("Domaine Nordique de Font-Romeu", D66, &["romeu"]),
    m("Domaine Nordique de Beille", D09, &["beille"]),
    m("Domaine Nordique du Chioula", D09, &["chioula"]),
    m("Domaine Nordique de l’Étang de Lers", D09, &["lers"]),
    m("Domaine Nordique de Mijanès", D09, &["mijanes"]),
];

const NATIONAL_PRICES: &[Price] = &[
    p(
        "saison",
        "adulte",
        None,
        214.0,
        "prevente",
        Some("2026-10-01"),
        Some("2026-11-15"),
        "nf-pass-national",
        "Promo du 1er octobre au 15 novembre = 214 €",
    ),
    p(
        "saison",
        "adulte",
        None,
        255.0,
        "plein_tarif",
        Some("2026-11-16"),
        None,
        "nf-pass-national",
        "Plein tarif à partir du 16 novembre = 255 €",
    ),
    p(
        "saison",
        "jeune",
        Some("5-15 ans révolus"),
        79.0,
        "prevente",
        Some("2026-10-01"),
        Some("2026-11-15"),
        "nf-pass-national",
        "Promo du 1er octobre au 15 novembre = 79 €",
    ),
    p(
        "saison",
        "jeune",
        Some("5-15 ans révolus"),
        94.0,
        "plein_tarif",
        Some("2026-11-16"),
        None,
        "nf-pass-national",
        "Plein tarif à partir du 16 novembre = 94 €",
    ),
];

const ENJ_PRICES: &[Price] = &[
    p(
        "saison",
        "adulte",
        Some("à partir de 16 ans"),
        112.0,
        "promo_flash",
        Some("2026-10-09"),
        Some("2026-10-19"),
        "enj-grille",
        "Vente FLASH : du vendredi 09 au lundi 19 octobre 2026 inclus 112 € 45 €",
    ),
    p(
        "saison",
        "jeune",
        Some("de 5 à 15 ans révolus"),
        45.0,
        "promo_flash",
        Some("2026-10-09"),
        Some("2026-10-19"),
        "enj-grille",
        "Vente FLASH : du vendredi 09 au lundi 19 octobre 2026 inclus 112 € 45 €",
    ),
    p(
        "saison",
        "adulte",
        Some("à partir de 16 ans"),
        135.0,
        "promo",
        Some("2026-10-20"),
        Some("2026-11-16"),
        "enj-grille",
        "Du mardi 20 octobre au lundi 16 novembre 2026 inclus 135 € 50 €",
    ),
    p(
        "saison",
        "jeune",
        Some("de 5 à 15 ans révolus"),
        50.0,
        "promo",
        Some("2026-10-20"),
        Some("2026-11-16"),
        "enj-grille",
        "Du mardi 20 octobre au lundi 16 novembre 2026 inclus 135 € 50 €",
    ),
    p(
        "saison",
        "adulte",
        Some("à partir de 16 ans"),
        173.0,
        "plein_tarif",
        Some("2026-11-17"),
        None,
        "enj-grille",
        "A partir du mardi 17 novembre 2026 173 € 70 €",
    ),
    p(
        "saison",
        "jeune",
        Some("de 5 à 15 ans révolus"),
        70.0,
        "plein_tarif",
        Some("2026-11-17"),
        None,
        "enj-grille",
        "A partir du mardi 17 novembre 2026 173 € 70 €",
    ),
];

const ENJ_HEBDO_PRICES: &[Price] = &[
    p(
        "hebdo",
        "adulte",
        Some("à partir de 16 ans"),
        51.0,
        "tarif_unique",
        None,
        None,
        "enj-grille",
        "Les Pass HEBDO NORDIQUE TARIF UNIQUE 51 € 33 €",
    ),
    p(
        "hebdo",
        "jeune",
        Some("de 5 à 15 ans révolus"),
        33.0,
        "tarif_unique",
        None,
        None,
        "enj-grille",
        "Les Pass HEBDO NORDIQUE TARIF UNIQUE 51 € 33 €",
    ),
];

const HSN_PRICES: &[Price] = &[
    p(
        "saison",
        "adulte",
        None,
        142.0,
        "prevente",
        Some("2026-10-01"),
        Some("2026-11-15"),
        "hsn-np74",
        "Préventes* 142€",
    ),
    p(
        "saison",
        "adulte",
        None,
        178.0,
        "plein_tarif",
        Some("2026-11-16"),
        None,
        "hsn-np74",
        "Normal 178€",
    ),
    p(
        "saison",
        "jeune",
        Some("de 5 à 15 ans révolus"),
        44.0,
        "prevente",
        Some("2026-10-01"),
        Some("2026-11-15"),
        "hsn-np74",
        "Préventes* 44€",
    ),
    p(
        "saison",
        "jeune",
        Some("de 5 à 15 ans révolus"),
        52.0,
        "plein_tarif",
        Some("2026-11-16"),
        None,
        "hsn-np74",
        "Normal 52€",
    ),
];

const NADS_PRICES: &[Price] = &[
    p(
        "saison",
        "adulte",
        Some("de 16 à 79 ans"),
        143.0,
        "prevente",
        None,
        Some("2026-11-15"),
        "nads-tarifs",
        "ADULTE Prévente (jusqu'au 15/11/2025) : 143 € / ADULTE Plein tarif : 205 €",
    ),
    p(
        "saison",
        "adulte",
        Some("de 16 à 79 ans"),
        205.0,
        "plein_tarif",
        Some("2026-11-16"),
        None,
        "nads-tarifs",
        "ADULTE Prévente (jusqu'au 15/11/2025) : 143 € / ADULTE Plein tarif : 205 €",
    ),
    p(
        "saison",
        "jeune",
        Some("de 5 à 15 ans"),
        72.0,
        "prevente",
        None,
        Some("2026-11-15"),
        "nads-tarifs",
        "JEUNE Prévente (jusqu'au 15/11/2025) : 72 € / JEUNE Plein tarif : 87 €",
    ),
    p(
        "saison",
        "jeune",
        Some("de 5 à 15 ans"),
        87.0,
        "plein_tarif",
        Some("2026-11-16"),
        None,
        "nads-tarifs",
        "JEUNE Prévente (jusqu'au 15/11/2025) : 72 € / JEUNE Plein tarif : 87 €",
    ),
    p(
        "saison",
        "senior",
        Some("de 75 à 79 ans"),
        102.0,
        "prevente",
        None,
        Some("2026-11-15"),
        "nads-tarifs",
        "SÉNIOR Prévente (jusqu'au 15/11/2025) : 102 € / SÉNIOR Plein Tarif : 143 €",
    ),
    p(
        "saison",
        "senior",
        Some("de 75 à 79 ans"),
        143.0,
        "plein_tarif",
        Some("2026-11-16"),
        None,
        "nads-tarifs",
        "SÉNIOR Prévente (jusqu'au 15/11/2025) : 102 € / SÉNIOR Plein Tarif : 143 €",
    ),
];

const NADS_WEEK_PRICES: &[Price] = &[
    p(
        "semaine",
        "adulte",
        Some("de 16 à 79 ans (et sénior)"),
        77.0,
        "tarif_unique",
        None,
        None,
        "nads-tarifs",
        "ADULTE et SÉNIOR : 77 € / JEUNE : 49 €",
    ),
    p(
        "semaine",
        "jeune",
        Some("de 5 à 15 ans"),
        49.0,
        "tarif_unique",
        None,
        None,
        "nads-tarifs",
        "ADULTE et SÉNIOR : 77 € / JEUNE : 49 €",
    ),
];

const ISERE_PRICES: &[Price] = &[
    p(
        "saison",
        "adulte",
        Some("nés de 1951 à 2006"),
        160.0,
        "prevente",
        Some("2026-10-13"),
        Some("2026-10-31"),
        "isere-pass",
        "Adulte (de 1951 à 2006) 160€ (175€)",
    ),
    p(
        "saison",
        "adulte",
        Some("nés de 1951 à 2006"),
        175.0,
        "plein_tarif",
        Some("2026-11-01"),
        None,
        "isere-pass",
        "Adulte (de 1951 à 2006) 160€ (175€)",
    ),
    p(
        "saison",
        "jeune",
        Some("nés de 2007 à 2022"),
        57.0,
        "prevente",
        Some("2026-10-13"),
        Some("2026-10-31"),
        "isere-pass",
        "Jeune (de 2007 à 2022) 57€ (62€)",
    ),
    p(
        "saison",
        "jeune",
        Some("nés de 2007 à 2022"),
        62.0,
        "plein_tarif",
        Some("2026-11-01"),
        None,
        "isere-pass",
        "Jeune (de 2007 à 2022) 57€ (62€)",
    ),
    p(
        "saison",
        "senior",
        Some("nés en 1950 et avant"),
        89.0,
        "prevente",
        Some("2026-10-13"),
        Some("2026-10-31"),
        "isere-pass",
        "Senior (1950 et avant) 89€ (97€)",
    ),
    p(
        "saison",
        "senior",
        Some("nés en 1950 et avant"),
        97.0,
        "plein_tarif",
        Some("2026-11-01"),
        None,
        "isere-pass",
        "Senior (1950 et avant) 89€ (97€)",
    ),
];

/// The titles.
pub const TITRES: &[Titre] = &[
    Titre {
        id: "nordic-pass-national",
        name: "Nordic Pass saison national",
        scope: "national",
        issuer: "Nordic France (vendu par les associations départementales ou de massif et les domaines)",
        products: &["saison"],
        validity: "Nordic France propose un titre national valable sur tous les sites nordiques adhérents",
        validity_doc: "nf-pass-national",
        notes: &[
            "« à l’exclusion des « nocturnes » et aux pistes ouvertes grâce au snowfarming, à la neige de culture, au ski sur glacier en amont du 15 décembre » (Nordic France)",
            "Hautes-Pyrénées et Pyrénées-Atlantiques non adhérentes : « tous, sauf Hautes-Pyrénées et Pyrénées Atlantiques » (Haute-Savoie Nordic)",
            "gratuit pour les moins de 5 ans (Haute-Savoie Nordic)",
        ],
        members: &[],
        abroad: Some(
            "Suisse romande (Romandie Ski de Fond) : « de skier gratuitement sur les domaines suisses » (Nordic France)",
        ),
        prices: NATIONAL_PRICES,
    },
    Titre {
        id: "enj-montagnes-du-jura-saison",
        name: "Pass Nordique saison Montagnes du Jura",
        scope: "massif",
        issuer: "Espace Nordique Jurassien",
        products: &["saison"],
        validity: "Pass saison Montagnes du Jura : accès à tous les sites nordiques de l'Ain, du Doubs et du Jura, ainsi qu'en Suisse Romande.",
        validity_doc: "enj-faq",
        notes: &[
            "grille 2026/2027 : « valable pour toutes les activités nordiques confondues (hors traineaux) sur les sites nordiques des Montagnes du Jura et adhérents à Romandie Ski de Fond (Suisse) »",
            "promo famille : Pass offert à partir de la 4e personne, jeunes de moins de 16 ans, 3 gratuités au plus (sauf Pass nationaux)",
            "pas de réduction sur les Nordic Pass journée de Haute-Savoie : « Il n’y a pas d’accord de réciprocité entre la Haute-Savoie et les Montagnes du Jura » (Haute-Savoie Nordic)",
        ],
        members: ENJ_MEMBERS,
        abroad: Some("Suisse romande : sites adhérents à Romandie Ski de Fond"),
        prices: ENJ_PRICES,
    },
    Titre {
        id: "enj-montagnes-du-jura-hebdo",
        name: "Pass Nordique hebdo Montagnes du Jura",
        scope: "massif",
        issuer: "Espace Nordique Jurassien",
        products: &["hebdo"],
        validity: "Le Pass hebdo Montagnes du Jura (ski et balade) est valable sur TOUS les sites nordiques du massif du Jura.",
        validity_doc: "enj-faq",
        notes: &["conseillé pour la Grande Traversée du Jura (séjours de 5 jours et plus)"],
        members: ENJ_MEMBERS,
        abroad: Some("Suisse romande : sites adhérents à Romandie Ski de Fond (grille 2026/2027)"),
        prices: ENJ_HEBDO_PRICES,
    },
    Titre {
        id: "hsn-nordic-pass-74",
        name: "Nordic Pass Haute-Savoie « le 74 »",
        scope: "departemental",
        issuer: "Haute-Savoie Nordic",
        products: &["saison"],
        validity: "« Le 74 » permet de skier tout l’hiver sur les 25 domaines nordiques de Haute-Savoie",
        validity_doc: "hsn-np74",
        notes: &[
            "accès aux nocturnes de Haute-Savoie depuis l’hiver 2025-2026",
            "réduction de 50 % sur l’accès journée en Suisse romande, de 40 à 50 % au Val d’Aoste",
        ],
        members: HSN_MEMBERS,
        abroad: None,
        prices: HSN_PRICES,
    },
    Titre {
        id: "nordic-isere-isere-drome",
        name: "Nordic Pass saison Isère-Drôme",
        scope: "regroupement",
        issuer: "Nordic Isère",
        products: &["saison"],
        validity: "Ce Nordic Pass saison est valable sur 23 sites de l’Isère et de la Drôm",
        validity_doc: "isere-pass",
        notes: &[
            "prévente du 13 au 31 octobre 2026 ; le prix entre parenthèses est le plein tarif",
        ],
        members: ISERE_DROME_MEMBERS,
        abroad: None,
        prices: ISERE_PRICES,
    },
    Titre {
        id: "nads-alpes-du-sud-saison",
        name: "Nordic Pass saison Alpes du Sud",
        scope: "massif",
        issuer: "Nordic Alpes du Sud",
        products: &["saison"],
        validity: "Nordic Pass Saison ALPES DU SUD",
        validity_doc: "nads-tarifs",
        notes: &[
            "validité : domaines adhérents de Nordic Alpes du Sud (04, 05, 06 ; « 32 DOMAINES NORDIQUES » sur la billetterie) ; la page des tarifs ne l’écrit pas en toutes lettres",
            "la page des tarifs écrit « Prévente (jusqu'au 15/11/2025) » sous l’en-tête 2026-2027 ; la grille PDF 2026-2027 donne les mêmes montants",
            "gratuit pour les moins de 5 ans et à partir de 80 ans",
        ],
        members: NADS_SHOP_MEMBERS,
        abroad: None,
        prices: NADS_PRICES,
    },
    Titre {
        id: "nads-alpes-du-sud-semaine",
        name: "Nordic Pass semaine Alpes du Sud",
        scope: "massif",
        issuer: "Nordic Alpes du Sud",
        products: &["semaine"],
        validity: "Nordic Pass semaine ALPES DU SUD",
        validity_doc: "nads-tarifs",
        notes: &[
            "validité : domaines adhérents de Nordic Alpes du Sud (déduite, comme le Pass saison)",
        ],
        members: NADS_SHOP_MEMBERS,
        abroad: None,
        prices: NADS_WEEK_PRICES,
    },
];

/// Members of the national pass: every association list of the Nordic France page.
pub fn national_members() -> Vec<Member> {
    let mut v: Vec<Member> = Vec::new();
    v.extend_from_slice(ENJ_MEMBERS);
    v.extend(HSN_MEMBERS.iter().copied());
    v.extend_from_slice(ISERE_DROME_MEMBERS);
    v.extend_from_slice(NADS_MEMBERS);
    v.extend_from_slice(OTHER_NF_MEMBERS);
    v
}

/// Rules of the site (day) passes.
pub const SITE_RULES: &[SiteRule] = &[
    SiteRule {
        depts: &["01", "25", "39"],
        rule: "Le Pass séance est valable uniquement sur le site d'achat du Pass. Une réciprocité existe quand plusieurs domaines peuvent être reliés ski aux pieds. Cette règle est la même pour les Pass pluri journaliers (de 2 à 5 jours).",
        doc: "enj-faq",
    },
    SiteRule {
        depts: &["74"],
        rule: "Nordic Pass hebdomadaire : « il contient une journée de réciprocité sur un autre domaine nordique » de Haute-Savoie ; Nordic Pass journée : sur le domaine d’achat.",
        doc: "hsn-np74",
    },
];

const CERVIERES_PRICES: &[Price] = &[
    Price {
        season: "2025-2026",
        ..p(
            "journee",
            "adulte",
            None,
            15.0,
            "tarif_unique",
            None,
            None,
            "nads-cervieres",
            "Journée Adulte:15€",
        )
    },
    Price {
        season: "2025-2026",
        ..p(
            "journee",
            "jeune",
            Some("5-15 ans"),
            8.0,
            "tarif_unique",
            None,
            None,
            "nads-cervieres",
            "Journée Jeune: 8€",
        )
    },
    Price {
        season: "2025-2026",
        ..p(
            "3h",
            "adulte",
            None,
            11.0,
            "tarif_unique",
            None,
            None,
            "nads-cervieres",
            "3H Adulte : 11€",
        )
    },
    Price {
        season: "2025-2026",
        ..p(
            "3h",
            "jeune",
            Some("5-15 ans"),
            6.0,
            "tarif_unique",
            None,
            None,
            "nads-cervieres",
            "3H Jeune: 6€",
        )
    },
    Price {
        season: "2025-2026",
        ..p(
            "saison_site",
            "adulte",
            None,
            75.0,
            "tarif_unique",
            None,
            None,
            "nads-cervieres",
            "Cervières:75€",
        )
    },
    Price {
        season: "2025-2026",
        ..p(
            "saison_site",
            "jeune",
            Some("5-15 ans"),
            60.0,
            "tarif_unique",
            None,
            None,
            "nads-cervieres",
            "Cervières:60€",
        )
    },
];

/// Prices of site titles read by hand.
pub const SITE_PRICES: &[SitePrices] = &[SitePrices {
    domains: &["net-152532950"],
    label: "Cervières - Izoard",
    prices: CERVIERES_PRICES,
}];
