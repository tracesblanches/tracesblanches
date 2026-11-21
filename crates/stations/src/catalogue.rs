//! Configuration of the title sources, and the few hand-made complements that no source
//! publishes as data.
//!
//! Everything else is read on the issuers' pages at every refresh ([`crate::sources`]).
//! Each complement carries its source, the date it was checked and an expiry date; the
//! test `complements_not_expired` fails once a complement has expired, and the pipeline
//! stops using it.
//!
//! Reuse: none of the issuers displays a reuse licence (Nordic France, ENJ, Haute-Savoie
//! Nordic, Nordic Alpes du Sud, Nordic Isère). Prices are facts and the extraction is
//! small, but a systematic extraction of a member list could fall under the database right
//! (Code de la propriété intellectuelle, art. L341-1, L342-1): titles are
//! `personal_use_only` until the issuers agree (letters L1 and L4).

/// A source document.
#[derive(Debug, Clone, Copy)]
pub struct Doc {
    /// Id.
    pub id: &'static str,
    /// Publisher.
    pub publisher: &'static str,
    /// Title.
    pub title: &'static str,
    /// URL.
    pub url: &'static str,
    /// Downloader source name.
    pub source: &'static str,
    /// `html` or `pdf`.
    pub format: &'static str,
}

/// Source documents.
pub const DOCS: &[Doc] = &[
    Doc {
        id: "nf-pass-national",
        publisher: "Nordic France",
        title: "Pass National (tarifs et liste des domaines accessibles)",
        url: "https://www.nordicfrance.fr/qui-sommes-nous/pass-national",
        source: "nordicfrance",
        format: "html",
    },
    Doc {
        id: "enj-faq",
        publisher: "Espace Nordique Jurassien",
        title: "FAQ de la vente en ligne des Pass",
        url: "https://pass.espacenordiquejurassien.com/faq",
        source: "enj",
        format: "html",
    },
    Doc {
        id: "enj-pass-saison",
        publisher: "Espace Nordique Jurassien",
        title: "Vente des Pass Saison : sites nordiques du Doubs, du Jura et de l'Ain",
        url: "https://pass.espacenordiquejurassien.com/home-pass-saison",
        source: "enj",
        format: "html",
    },
    Doc {
        id: "enj-grille",
        publisher: "Espace Nordique Jurassien",
        title: "Grille tarifaire Pass Nordique saison 2026/2027 (PDF)",
        url: "https://www.espacenordiquejurassien.com/csx/scripts/downloader2.php?filename=T004/fichier/fa/2e/FR.fw6mt6wepbn&mime=application%2Fpdf&originalname=GRILLE_TARIFAIRE_2026-2027.pdf",
        source: "enj",
        format: "pdf",
    },
    Doc {
        id: "hsn-np74",
        publisher: "Haute-Savoie Nordic",
        title: "Les différents Nordic Pass et les tarifs",
        url: "https://www.haute-savoie-nordic.com/forfait-nordic-pass-74/",
        source: "haute-savoie-nordic",
        format: "html",
    },
    Doc {
        id: "nads-tarifs",
        publisher: "Nordic Alpes du Sud",
        title: "Nordic Pass - Tarifs",
        url: "https://www.nordicalpesdusud.com/nordic-pass/tarifs-2026-2027",
        source: "nordicalpesdusud",
        format: "html",
    },
    Doc {
        id: "nads-billetterie",
        publisher: "Nordic Alpes du Sud",
        title: "Billetterie nordique (domaines où acheter un Nordic Pass)",
        url: "https://billetterie-nordic-nads.intence.tech/",
        source: "nads-billetterie",
        format: "html",
    },
    Doc {
        id: "isere-pass",
        publisher: "Nordic Isère",
        title: "Pass Isère-Drôme",
        url: "https://www.nordic-isere.fr/pass-isere/",
        source: "nordic-isere",
        format: "html",
    },
];

/// Root of the Nordic Alpes du Sud site: the domain pages are found from its menu
/// (`/domaines-nordiques/<area>`, then `/domaines/<slug>`).
pub const NADS_ROOT: &str = "https://www.nordicalpesdusud.com";

/// One price row of a [`PdfPrices`]: product, category, ages, euros, tier, from, until,
/// quote.
pub type PdfRow = (
    &'static str,
    &'static str,
    &'static str,
    f64,
    &'static str,
    Option<&'static str>,
    Option<&'static str>,
    &'static str,
);

/// Prices only published in a PDF that the pipeline cannot read (no PDF reader in the
/// workspace): typed from the document, pinned to its SHA-256 (a changed document is not
/// trusted: the prices are withdrawn and the source flagged) and to an expiry date.
#[derive(Debug, Clone, Copy)]
pub struct PdfPrices {
    /// Title id.
    pub titre: &'static str,
    /// Document id.
    pub doc: &'static str,
    /// SHA-256 of the document read.
    pub sha256: &'static str,
    /// Date read.
    pub checked_on: &'static str,
    /// Last day the prices may be used.
    pub expires: &'static str,
    /// Season.
    pub season: &'static str,
    /// Rows.
    pub rows: &'static [PdfRow],
}

const ENJ_SHA: &str = "46d4b3f1710cf9ca41e8d8fb5e68902713354ab692e0043f98d9d6aee00a0afb";

/// The Montagnes du Jura prices of the ENJ grid 2026/2027.
pub const PDF_PRICES: &[PdfPrices] = &[
    PdfPrices {
        titre: "enj-montagnes-du-jura-saison",
        doc: "enj-grille",
        sha256: ENJ_SHA,
        checked_on: "2026-11-15",
        expires: "2027-04-30",
        season: "2026-2027",
        rows: &[
            (
                "saison",
                "adulte",
                "à partir de 16 ans",
                112.0,
                "promo_flash",
                Some("2026-10-09"),
                Some("2026-10-19"),
                "Vente FLASH : du vendredi 09 au lundi 19 octobre 2026 inclus 112 € 45 €",
            ),
            (
                "saison",
                "jeune",
                "de 5 à 15 ans révolus",
                45.0,
                "promo_flash",
                Some("2026-10-09"),
                Some("2026-10-19"),
                "Vente FLASH : du vendredi 09 au lundi 19 octobre 2026 inclus 112 € 45 €",
            ),
            (
                "saison",
                "adulte",
                "à partir de 16 ans",
                135.0,
                "promo",
                Some("2026-10-20"),
                Some("2026-11-16"),
                "Du mardi 20 octobre au lundi 16 novembre 2026 inclus 135 € 50 €",
            ),
            (
                "saison",
                "jeune",
                "de 5 à 15 ans révolus",
                50.0,
                "promo",
                Some("2026-10-20"),
                Some("2026-11-16"),
                "Du mardi 20 octobre au lundi 16 novembre 2026 inclus 135 € 50 €",
            ),
            (
                "saison",
                "adulte",
                "à partir de 16 ans",
                173.0,
                "plein_tarif",
                Some("2026-11-17"),
                None,
                "A partir du mardi 17 novembre 2026 173 € 70 €",
            ),
            (
                "saison",
                "jeune",
                "de 5 à 15 ans révolus",
                70.0,
                "plein_tarif",
                Some("2026-11-17"),
                None,
                "A partir du mardi 17 novembre 2026 173 € 70 €",
            ),
        ],
    },
    PdfPrices {
        titre: "enj-montagnes-du-jura-hebdo",
        doc: "enj-grille",
        sha256: ENJ_SHA,
        checked_on: "2026-11-15",
        expires: "2027-04-30",
        season: "2026-2027",
        rows: &[
            (
                "hebdo",
                "adulte",
                "à partir de 16 ans",
                51.0,
                "tarif_unique",
                None,
                None,
                "Les Pass HEBDO NORDIQUE TARIF UNIQUE 51 € 33 €",
            ),
            (
                "hebdo",
                "jeune",
                "de 5 à 15 ans révolus",
                33.0,
                "tarif_unique",
                None,
                None,
                "Les Pass HEBDO NORDIQUE TARIF UNIQUE 51 € 33 €",
            ),
        ],
    },
];

/// Census domains of a member that the name matching cannot find (a commune, a piste
/// name or a network that the source does not name), checked by hand.
#[derive(Debug, Clone, Copy)]
pub struct MemberOverride {
    /// Words that the folded member label must contain ([`crate::sources::fold`]).
    pub label_contains: &'static str,
    /// Census domains added to the matching.
    pub domains: &'static [&'static str],
    /// `verifie`, `partiel` (the domain also holds trails of non-member sites), `probable`.
    pub confidence: &'static str,
    /// Why (source).
    pub reason: &'static str,
    /// Date checked.
    pub checked_on: &'static str,
    /// Expiry: the census or the member lists may have changed by then.
    pub expires: &'static str,
}

const fn o(
    label_contains: &'static str,
    domains: &'static [&'static str],
    confidence: &'static str,
    reason: &'static str,
) -> MemberOverride {
    MemberOverride {
        label_contains,
        domains,
        confidence,
        reason,
        checked_on: "2026-11-15",
        expires: "2027-06-30",
    }
}

/// Hand-checked complements of the member matching.
pub const OVERRIDES: &[MemberOverride] = &[
    o(
        "agy",
        &["net-196901574"],
        "verifie",
        "Agy est à Saint-Sigismond (rapport 08)",
    ),
    o(
        "orange",
        &["net-1012797281"],
        "verifie",
        "Orange : réseau de La Roche-sur-Foron (rapport 08)",
    ),
    o(
        "alpe du grand serre",
        &["net-1133481879"],
        "verifie",
        "Alpe du Grand Serre : La Morte (rapport 08)",
    ),
    o(
        "coulmes",
        &["net-78675377"],
        "verifie",
        "Les Coulmes : Rencurel (rapport 08)",
    ),
    o(
        "villard de lans",
        &["site-2798243"],
        "probable",
        "le domaine « Espace Nordique du Haut vercors » porte Villard-de-Lans et Corrençon",
    ),
    o(
        "draye",
        &["net-66410529"],
        "verifie",
        "le domaine « Crots » porte la piste « Boucle Le sentier de la Draye »",
    ),
    o(
        "bresse lispach",
        &["net-22890557"],
        "partiel",
        "le réseau « Gérardmer » porte « Liaison La Bresse / Xonrupt / Gérardmer » ; Gérardmer et Xonrupt ne sont pas dans la liste",
    ),
    o(
        "combe saint pierre",
        &["net-206107197"],
        "probable",
        "Combe Saint-Pierre (Charquemont) : réseau de Fournet-Blancheroche",
    ),
    o(
        "plateau de maiche",
        &["net-914473998", "net-925024307"],
        "probable",
        "Plateau de Maîche : Damprichard ; Plateau du Russey : Le Bélieu",
    ),
    o(
        "lacs et montagnes du h",
        &[
            "site-2764548",
            "site-3161203",
            "site-2783351",
            "site-13681086",
            "net-920680329",
        ],
        "verifie",
        "boutique ENJ : points de vente dans les villages des domaines de l'Espace nordique jurassien (Les Fourgs, Métabief, Mouthe, Chaux-Neuve…)",
    ),
    o(
        "grand pontarlier",
        &["site-13767199", "site-3533848"],
        "verifie",
        "boutique ENJ : Le Gounefay - le Larmont, La Malmaison",
    ),
    o(
        "val de morteau",
        &["net-159224166"],
        "verifie",
        "boutique ENJ : Les Combes",
    ),
    o(
        "val de vennes",
        &["net-185284964"],
        "verifie",
        "boutique ENJ : Fournets-Luisans",
    ),
    o(
        "monts jura",
        &["site-3161151", "net-1021944915"],
        "verifie",
        "boutique ENJ : La Vattay - Mijoux - la Valserine - Menthières",
    ),
    o(
        "montbenoit",
        &["net-123797916", "net-97559764"],
        "verifie",
        "Arc-sous-Cicon, Chaux de Gilley",
    ),
    o(
        "hautes combes du jura",
        &["site-13699336", "site-13705091", "site-13705382"],
        "verifie",
        "boutique ENJ : Lajoux - Les Moussières - La Pesse",
    ),
    o(
        "bellefontaine",
        &["site-3161183"],
        "verifie",
        "boutique ENJ : « Village - les Lacs - Risoux »",
    ),
    o(
        "nanchez",
        &["site-13134084"],
        "probable",
        "Saint-Pierre porte les pistes de Nanchez (OpenSkiMap « Nanchez »)",
    ),
    o(
        "sur lyand",
        &["site-9227542"],
        "verifie",
        "Sur Lyand (Ain, Grand Colombier) est aussi dans le réseau Haute-Savoie Nordic",
    ),
    o(
        "herbouilly",
        &["net-87271071"],
        "probable",
        "Herbouilly est à Saint-Agnan-en-Vercors",
    ),
    o(
        "saint hugues",
        &["net-155671256"],
        "verifie",
        "Nordic Isère : « Saint Hugues (Saint Pierre de Chartreuse) »",
    ),
    o(
        "saisies",
        &["site-5993384"],
        "probable",
        "Les Saisies font partie de l'Espace Diamant",
    ),
    o(
        "prat de bouc",
        &["net-4602780"],
        "probable",
        "Prat de Bouc est à Albepierre-Bredons, commune du domaine « Le Lioran »",
    ),
    o(
        "mezenc",
        &["net-72443280"],
        "probable",
        "Les Estables : station du Mézenc",
    ),
    o(
        "montagne ardechoise",
        &["net-162538644"],
        "probable",
        "La Chavade Bel Air : domaine de la Montagne ardéchoise",
    ),
    o(
        "col de la loge",
        &["net-50694408"],
        "probable",
        "Col de la Loge : réseau de Chalmazel-Jeansagnière",
    ),
];

/// Whether a complement expiring on `expires` (`YYYY-MM-DD`) is still usable on `today`.
pub fn usable(expires: &str, today: &str) -> bool {
    today <= expires
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fails once a hand-made complement has expired: check it again against its source
    /// and move its expiry date, or delete it.
    #[test]
    fn complements_not_expired() {
        let today = jiff::Timestamp::now().strftime("%Y-%m-%d").to_string();
        for p in PDF_PRICES {
            assert!(
                usable(p.expires, &today),
                "prix {} expirés le {}",
                p.titre,
                p.expires
            );
        }
        for o in OVERRIDES {
            assert!(
                usable(o.expires, &today),
                "complément « {} » expiré le {}",
                o.label_contains,
                o.expires
            );
        }
    }

    #[test]
    fn complements_are_sourced() {
        for p in PDF_PRICES {
            assert!(DOCS.iter().any(|d| d.id == p.doc && d.format == "pdf"));
            assert_eq!(p.sha256.len(), 64);
            assert!(!p.rows.is_empty());
        }
        for o in OVERRIDES {
            assert!(!o.reason.is_empty() && !o.domains.is_empty());
            assert!(
                o.domains
                    .iter()
                    .all(|d| d.starts_with("site-") || d.starts_with("net-"))
            );
        }
        assert!(usable("2027-06-30", "2027-06-30") && !usable("2027-06-30", "2027-07-01"));
    }
}
