//! Colours, symbols and legend of the map overlays.
//!
//! Information is never carried by colour alone (K12, #115): each piste difficulty has
//! its own shape along the line, one-way pistes carry arrows, piste status has a cross or
//! a tick, refuges a house, and every legend row has a French label. Colours are dark and
//! saturated so that they keep a contrast of at least 4.5:1 on snow white (`#ffffff`),
//! checked by the tests below.

use crate::info::PackInfo;

/// Shape of a map symbol, drawn by the app.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum SymbolShape {
    /// Filled disc (green piste).
    Circle,
    /// Filled square (blue piste).
    Square,
    /// Filled diamond (red piste).
    Diamond,
    /// Two diamonds side by side (black piste).
    DoubleDiamond,
    /// Hollow ring (difficulty unknown).
    Ring,
    /// Arrow pointing right, along the line (one-way piste).
    Arrow,
    /// Cross on a disc (closed).
    Cross,
    /// Tick on a disc (open).
    Check,
    /// House on a rounded square (refuge, hut, shelter).
    House,
    /// Water drop on a disc (water point).
    Drop,
}

/// A symbol image of the map style.
///
/// For [`SymbolShape::Cross`], [`SymbolShape::Check`] and [`SymbolShape::House`],
/// `fill` is the background (disc or rounded square) and `stroke` the glyph drawn on it;
/// for the other shapes `fill` is the shape and `stroke` its outline.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct MapSymbol {
    /// Image identifier used by the style (`icon-image`).
    pub id: String,
    /// Shape.
    pub shape: SymbolShape,
    /// Fill colour `#rrggbb`.
    pub fill: String,
    /// Outline or glyph colour `#rrggbb`.
    pub stroke: String,
    /// Size of the image in density-independent pixels.
    pub size_dp: u32,
}

/// One row of the legend.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct LegendEntry {
    /// Group title, French (`Pistes`, `État des pistes`, `Refuges`).
    pub group: String,
    /// Label, French.
    pub label: String,
    /// Symbol drawn next to the label ([`MapSymbol::id`]), if any.
    pub symbol: Option<String>,
    /// Colour of the line sample `#rrggbb`, if the row shows a line.
    pub line_color: Option<String>,
    /// Whether the line sample is dashed.
    pub line_dashed: bool,
}

/// Piste difficulty as written by the `osm` crate (`difficulty` property).
pub(crate) struct Difficulty {
    pub(crate) value: &'static str,
    pub(crate) label: &'static str,
    pub(crate) color: &'static str,
    pub(crate) shape: SymbolShape,
}

/// The four piste colours of French nordic areas, darkened for contrast on snow.
pub(crate) const DIFFICULTIES: [Difficulty; 4] = [
    Difficulty {
        value: "novice",
        label: "Verte : très facile",
        color: "#00703c",
        shape: SymbolShape::Circle,
    },
    Difficulty {
        value: "easy",
        label: "Bleue : facile",
        color: "#0047ab",
        shape: SymbolShape::Square,
    },
    Difficulty {
        value: "intermediate",
        label: "Rouge : difficile",
        color: "#c8102e",
        shape: SymbolShape::Diamond,
    },
    Difficulty {
        value: "advanced",
        label: "Noire : très difficile",
        color: "#111111",
        shape: SymbolShape::DoubleDiamond,
    },
];

/// Colour of a piste whose difficulty is unknown.
pub(crate) const UNKNOWN_COLOR: &str = "#5c5c5c";
/// Casing drawn under the piste lines and halo of the labels.
pub(crate) const CASING_COLOR: &str = "#ffffff";
/// Arrows of one-way pistes.
pub(crate) const ARROW_COLOR: &str = "#111111";
/// Closed itineraries (line and cross disc).
pub(crate) const CLOSED_COLOR: &str = "#a00000";
/// Open itineraries (halo and tick disc).
pub(crate) const OPEN_COLOR: &str = "#006b3c";
/// Refuges (house square and labels).
pub(crate) const REFUGE_COLOR: &str = "#5b2c06";
/// Water points (drop disc).
pub(crate) const WATER_COLOR: &str = "#0b4f8a";
/// Areas without any mobile network (fill and dashed outline).
pub(crate) const NO_NETWORK_COLOR: &str = "#4a1486";

/// Symbol id of a difficulty value (`difficulty-novice`...).
pub(crate) fn difficulty_symbol(value: &str) -> String {
    format!("skimap-difficulty-{value}")
}
/// Symbol id of an unknown difficulty.
pub(crate) const UNKNOWN_SYMBOL: &str = "skimap-difficulty-unknown";
/// Symbol id of the one-way arrow.
pub(crate) const ARROW_SYMBOL: &str = "skimap-oneway";
/// Symbol id of a closed itinerary.
pub(crate) const CLOSED_SYMBOL: &str = "skimap-status-closed";
/// Symbol id of an open itinerary.
pub(crate) const OPEN_SYMBOL: &str = "skimap-status-open";
/// Symbol id of a refuge.
pub(crate) const REFUGE_SYMBOL: &str = "skimap-refuge";
/// Symbol id of a water point.
pub(crate) const WATER_SYMBOL: &str = "skimap-water";

pub(crate) fn map_symbols() -> Vec<MapSymbol> {
    let mut v: Vec<MapSymbol> = DIFFICULTIES
        .iter()
        .map(|d| MapSymbol {
            id: difficulty_symbol(d.value),
            shape: d.shape,
            fill: d.color.to_owned(),
            stroke: CASING_COLOR.to_owned(),
            size_dp: 18,
        })
        .collect();
    let mut push = |id: &str, shape, fill: &str, stroke: &str, size_dp| {
        v.push(MapSymbol {
            id: id.to_owned(),
            shape,
            fill: fill.to_owned(),
            stroke: stroke.to_owned(),
            size_dp,
        });
    };
    push(
        UNKNOWN_SYMBOL,
        SymbolShape::Ring,
        UNKNOWN_COLOR,
        CASING_COLOR,
        16,
    );
    push(
        ARROW_SYMBOL,
        SymbolShape::Arrow,
        ARROW_COLOR,
        CASING_COLOR,
        20,
    );
    push(
        CLOSED_SYMBOL,
        SymbolShape::Cross,
        CLOSED_COLOR,
        CASING_COLOR,
        22,
    );
    push(
        OPEN_SYMBOL,
        SymbolShape::Check,
        OPEN_COLOR,
        CASING_COLOR,
        22,
    );
    push(
        REFUGE_SYMBOL,
        SymbolShape::House,
        REFUGE_COLOR,
        CASING_COLOR,
        28,
    );
    push(
        WATER_SYMBOL,
        SymbolShape::Drop,
        WATER_COLOR,
        CASING_COLOR,
        22,
    );
    v
}

pub(crate) fn legend(info: &PackInfo) -> Vec<LegendEntry> {
    let has = |id: &str| info.overlays.iter().any(|o| o.id == id);
    let mut v = Vec::new();
    let row = |group: &str, label: &str, symbol: Option<String>, line: Option<&str>, dashed| {
        LegendEntry {
            group: group.to_owned(),
            label: label.to_owned(),
            symbol,
            line_color: line.map(str::to_owned),
            line_dashed: dashed,
        }
    };
    if has("pistes") {
        for d in &DIFFICULTIES {
            v.push(row(
                "Pistes",
                d.label,
                Some(difficulty_symbol(d.value)),
                Some(d.color),
                false,
            ));
        }
        v.push(row(
            "Pistes",
            "Difficulté inconnue",
            Some(UNKNOWN_SYMBOL.to_owned()),
            Some(UNKNOWN_COLOR),
            false,
        ));
        v.push(row(
            "Pistes",
            "Pointillés : raquettes et piétons",
            None,
            Some(UNKNOWN_COLOR),
            true,
        ));
        v.push(row(
            "Pistes",
            "Flèches : sens unique",
            Some(ARROW_SYMBOL.to_owned()),
            None,
            false,
        ));
    }
    if has("status") {
        v.push(row(
            "État des pistes",
            "Croix : itinéraire fermé",
            Some(CLOSED_SYMBOL.to_owned()),
            Some(CLOSED_COLOR),
            true,
        ));
        v.push(row(
            "État des pistes",
            "Coche : itinéraire ouvert",
            Some(OPEN_SYMBOL.to_owned()),
            Some(OPEN_COLOR),
            false,
        ));
    } else {
        v.push(row(
            "État des pistes",
            "État des pistes absent de ce pack",
            None,
            None,
            false,
        ));
    }
    if ["refuges-info", "refuges-osm", "refuges-camptocamp"]
        .iter()
        .any(|id| has(id))
    {
        v.push(row(
            "Refuges",
            "Refuge, cabane ou abri",
            Some(REFUGE_SYMBOL.to_owned()),
            None,
            false,
        ));
    }
    if has("refuges-info") {
        v.push(row(
            "Refuges",
            "Goutte : point d'eau",
            Some(WATER_SYMBOL.to_owned()),
            None,
            false,
        ));
    }
    if has("no-network") {
        v.push(row(
            "Réseau",
            "Tirets violets : aucun réseau mobile, 112 compris",
            None,
            Some(NO_NETWORK_COLOR),
            true,
        ));
    }
    v
}

/// WCAG relative luminance of `#rrggbb`.
#[cfg(test)]
fn luminance(hex: &str) -> f64 {
    let c = |i: usize| {
        let v = f64::from(u8::from_str_radix(&hex[i..i + 2], 16).unwrap()) / 255.0;
        if v <= 0.03928 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * c(1) + 0.7152 * c(3) + 0.0722 * c(5)
}

/// WCAG contrast ratio of two `#rrggbb` colours.
#[cfg(test)]
pub(crate) fn contrast(a: &str, b: &str) -> f64 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_contrast_on_snow() {
        let colours = DIFFICULTIES.iter().map(|d| d.color).chain([
            UNKNOWN_COLOR,
            ARROW_COLOR,
            CLOSED_COLOR,
            OPEN_COLOR,
            REFUGE_COLOR,
            WATER_COLOR,
            NO_NETWORK_COLOR,
        ]);
        for c in colours {
            let r = contrast(c, "#ffffff");
            assert!(r >= 4.5, "{c} on white: {r:.2}");
        }
        // Green and black text halo: white glyphs on every disc stay readable.
        assert!(contrast(OPEN_COLOR, CASING_COLOR) >= 4.5);
        // Reference values of the formula.
        assert!((contrast("#000000", "#ffffff") - 21.0).abs() < 1e-9);
        assert!((contrast("#777777", "#ffffff") - 4.48).abs() < 0.01);
    }

    #[test]
    fn every_difficulty_has_its_own_shape() {
        let mut shapes: Vec<_> = DIFFICULTIES.iter().map(|d| d.shape).collect();
        shapes.push(SymbolShape::Ring);
        for (i, a) in shapes.iter().enumerate() {
            for b in &shapes[i + 1..] {
                assert_ne!(a, b);
            }
        }
    }

    #[test]
    fn symbol_ids_are_unique() {
        let s = map_symbols();
        let mut ids: Vec<_> = s.iter().map(|m| m.id.as_str()).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), s.len());
        assert_eq!(s.len(), 10);
    }
}
