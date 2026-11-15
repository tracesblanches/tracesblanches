//! Colours of the two themes. Light (`clair`) is for daylight on snow: white ground,
//! saturated dark-enough colours, white casings and halos. Dark (`sombre`) is for dusk
//! and night: near-black ground, brightened colours, dark casings and halos.

/// Theme of a style.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Theme {
    /// Light theme (`clair`).
    Clair,
    /// Dark theme (`sombre`).
    Sombre,
}

impl Theme {
    /// Both themes.
    pub const ALL: [Theme; 2] = [Theme::Clair, Theme::Sombre];

    /// Name used in file names and metadata.
    pub fn name(self) -> &'static str {
        match self {
            Theme::Clair => "clair",
            Theme::Sombre => "sombre",
        }
    }

    /// Palette of the theme.
    pub fn palette(self) -> &'static Palette {
        match self {
            Theme::Clair => &CLAIR,
            Theme::Sombre => &SOMBRE,
        }
    }
}

/// Colours (`#rrggbb`) of a theme.
#[derive(Debug)]
pub struct Palette {
    /// Ground (snow).
    pub ground: &'static str,
    /// Built-up areas.
    pub built: &'static str,
    /// Buildings.
    pub building: &'static str,
    /// Closed forest, faint (fond, guidage).
    pub forest_faint: &'static str,
    /// Closed forest, marked (SRN).
    pub forest: &'static str,
    /// Open forest and heath (SRN).
    pub forest_open: &'static str,
    /// Rock and scree.
    pub rock: &'static str,
    /// Rock outlines (SRN).
    pub rock_line: &'static str,
    /// Glacier.
    pub glacier: &'static str,
    /// Water surfaces.
    pub water: &'static str,
    /// Streams.
    pub stream: &'static str,
    /// Contour lines, faint (fond, guidage).
    pub contour_faint: &'static str,
    /// Contour lines, marked (SRN).
    pub contour: &'static str,
    /// Index contours and their labels (SRN).
    pub contour_index: &'static str,
    /// Minor roads.
    pub road_minor: &'static str,
    /// Major roads.
    pub road_major: &'static str,
    /// Footpaths and tracks (SRN).
    pub path: &'static str,
    /// Ski lifts.
    pub lift: &'static str,
    /// Main text.
    pub text: &'static str,
    /// Secondary text (contour heights, lieux-dits).
    pub text_soft: &'static str,
    /// Text halo and line casings.
    pub halo: &'static str,
    /// Piste colours: green, blue, red, black, unknown.
    pub piste: [&'static str; 5],
    /// Casing of the black piste (dark theme: light, so that it shows on dark ground).
    pub piste_black_casing: &'static str,
    /// Snowshoe and foot trails.
    pub hike: &'static str,
    /// Closed piste overlay (state).
    pub closed: &'static str,
    /// Band of a piste whose state is unknown (neutral: never drawn like an open piste).
    pub unknown: &'static str,
    /// Band of a black piste whose state is unknown (contrast with its black core).
    pub unknown_black: &'static str,
    /// Avalanche exposure on pistes: exposed, very exposed.
    pub exposure: [&'static str; 2],
    /// Avalanche exposure polygons: exposed, very exposed.
    pub exposure_fill: [&'static str; 2],
    /// Opacity of the exposure polygons.
    pub exposure_opacity: f64,
    /// Slope polygons: 30-35, 35-40, 40-45, 45° and more (IGN slope map hues).
    pub slope: [&'static str; 4],
    /// Opacity of the slope polygons.
    pub slope_opacity: f64,
    /// Historic avalanche areas (CLPA): outline in a hue of its own, apart from the reds
    /// of the slope classes it crosses.
    pub avalanche_area: &'static str,
    /// SRN itineraries and play areas outside the groomed domains.
    pub srn: &'static str,
    /// Snow classes: insufficient, borderline, sufficient.
    pub snow: [&'static str; 3],
    /// Opacity of the snow polygons.
    pub snow_opacity: f64,
    /// Trail-pass validity zones (outline, discreet).
    pub fee: &'static str,
    /// Zone where the pass of the user is valid (feature state `valid` true).
    pub fee_valid: &'static str,
    /// Zone of another pass (feature state `valid` false).
    pub fee_other: &'static str,
    /// Rescue post marker.
    pub rescue: &'static str,
    /// Sale point of the trail pass.
    pub sale: &'static str,
    /// Protected zones relevant in winter (quiet zones, APPB, reserves).
    pub zone: &'static str,
    /// Bivouac forbidden outline.
    pub bivouac: &'static str,
    /// No mobile network.
    pub no_network: &'static str,
    /// Parking marker.
    pub parking: &'static str,
    /// Nordic centre (foyer, ticket office) marker.
    pub centre: &'static str,
    /// Hut dot (domain views).
    pub hut: &'static str,
    /// Hut marker ground (dark theme draws a light disc under the IGN icons).
    pub hut_disc: &'static str,
    /// Water point.
    pub water_point: &'static str,
    /// Hazard (passage délicat).
    pub hazard: &'static str,
    /// Route of the day (guidage).
    pub route: &'static str,
    /// Part of the route already done (guidage).
    pub route_done: &'static str,
    /// Next decision marker (guidage).
    pub decision: &'static str,
    /// Next decision outline (guidage).
    pub decision_line: &'static str,
    /// Position dot.
    pub position: &'static str,
    /// Context pistes in guidage.
    pub piste_context: &'static str,
}

/// Light theme.
pub static CLAIR: Palette = Palette {
    ground: "#ffffff",
    built: "#f1eeea",
    building: "#cfc8bd",
    forest_faint: "#e4efdc",
    forest: "#c6dfb5",
    forest_open: "#e2eed6",
    rock: "#ebe8e4",
    rock_line: "#a8a29a",
    glacier: "#e3f1fb",
    water: "#9fd0f2",
    stream: "#4f9fd9",
    contour_faint: "#d6c6ad",
    contour: "#b69772",
    contour_index: "#8a6a43",
    road_minor: "#c9c2b8",
    road_major: "#a39887",
    path: "#7d5a35",
    lift: "#4a4a4a",
    text: "#1b1f24",
    text_soft: "#5a4632",
    halo: "#ffffff",
    piste: ["#008c45", "#0062c4", "#d81e1e", "#111111", "#6b6b6b"],
    piste_black_casing: "#ffffff",
    hike: "#6a5f55",
    closed: "#ffffff",
    unknown: "#dadee2",
    unknown_black: "#c3c8cd",
    exposure: ["#d06a00", "#c4001a"],
    exposure_fill: ["#f59628", "#c81e1e"],
    exposure_opacity: 0.3,
    slope: ["#f2e500", "#f39419", "#f00000", "#c889bb"],
    slope_opacity: 0.55,
    avalanche_area: "#1d2b64",
    srn: "#00838f",
    snow: ["#a0522d", "#7fb6e6", "#1f5fbf"],
    snow_opacity: 0.45,
    fee: "#6d6875",
    fee_valid: "#2e7d32",
    fee_other: "#a15c00",
    rescue: "#d50000",
    sale: "#00796b",
    zone: "#c24e00",
    bivouac: "#7b2d9b",
    no_network: "#5b2a86",
    parking: "#1f5fbf",
    centre: "#00796b",
    hut: "#8a4b08",
    hut_disc: "#ffffff",
    water_point: "#1f78d1",
    hazard: "#c4001a",
    route: "#c8007a",
    route_done: "#9a9a9a",
    decision: "#ffd400",
    decision_line: "#111111",
    position: "#0a6cff",
    piste_context: "#8e8e8e",
};

/// Dark theme.
pub static SOMBRE: Palette = Palette {
    ground: "#11151a",
    built: "#1b2027",
    building: "#3a4048",
    forest_faint: "#17221b",
    forest: "#1f3326",
    forest_open: "#1a2a20",
    rock: "#22252a",
    rock_line: "#6e6a64",
    glacier: "#1a2a38",
    water: "#1d4a73",
    stream: "#3f87c7",
    contour_faint: "#3a3329",
    contour: "#6f5c45",
    contour_index: "#a88c66",
    road_minor: "#3b4149",
    road_major: "#59616c",
    path: "#b08a5c",
    lift: "#b5b5b5",
    text: "#eef2f6",
    text_soft: "#d2c3ae",
    halo: "#0b0e12",
    piste: ["#2fd57a", "#4ea8ff", "#ff5a4f", "#000000", "#a3a3a3"],
    piste_black_casing: "#e0e0e0",
    hike: "#b9ab9c",
    closed: "#11151a",
    unknown: "#3a3f46",
    unknown_black: "#9aa0a6",
    exposure: ["#ff9f1c", "#ff3b4e"],
    exposure_fill: ["#ff9f1c", "#ff2d44"],
    exposure_opacity: 0.38,
    slope: ["#e6d200", "#ff8c1a", "#ff3b3b", "#c98bff"],
    slope_opacity: 0.78,
    avalanche_area: "#9fe7ff",
    srn: "#3fd0e0",
    snow: ["#c98a5a", "#3d7fc4", "#a9d4ff"],
    snow_opacity: 0.6,
    fee: "#b8b0c4",
    fee_valid: "#66bb6a",
    fee_other: "#ffb74d",
    rescue: "#ff5252",
    sale: "#2ec4b6",
    zone: "#ff8c3a",
    bivouac: "#c77dff",
    no_network: "#b388ff",
    parking: "#2f6fdb",
    centre: "#2ec4b6",
    hut: "#f0b878",
    hut_disc: "#f2f2f2",
    water_point: "#5bb0ff",
    hazard: "#ff3b4e",
    route: "#ff4fd8",
    route_done: "#6f6f6f",
    decision: "#ffd400",
    decision_line: "#111111",
    position: "#4ea8ff",
    piste_context: "#7b8088",
};
