//! Pack manifest (`manifest.json`), format version 1.
//!
//! Every path in the manifest is relative to the pack root, with `/` separators. Readers
//! reject a manifest whose `format` is not [`FORMAT`] or whose `format_version` is newer
//! than [`FORMAT_VERSION`].

use serde::{Deserialize, Serialize};
use skimap_common::site::SiteRecord;

/// Value of [`Manifest::format`].
pub const FORMAT: &str = "skimap-pack";
/// Format version written by this crate; readers accept versions up to it.
pub const FORMAT_VERSION: u32 = 1;
/// File name of the manifest at the pack root.
pub const MANIFEST_FILE: &str = "manifest.json";

/// The manifest of a pack.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    /// Always [`FORMAT`].
    pub format: String,
    /// Format version, [`FORMAT_VERSION`] for packs written by this crate.
    pub format_version: u32,
    /// Version of this pack: UTC build time `AAAAMMJJTHHMMSSZ`, increasing.
    pub pack_version: String,
    /// Writer, e.g. `skimap-pack 0.1.0`.
    pub generator: String,
    /// UTC build time, RFC 3339.
    pub generated_at: String,
    /// Site of the pack.
    pub site: SiteInfo,
    /// Validity of the pack as a whole (the earliest of its layers).
    pub valid_for: PackValidity,
    /// Whether layers from personal-use-only sources were included.
    pub personal_sources: bool,
    /// Basemaps (vector tiles + MapLibre style).
    pub basemaps: Vec<Basemap>,
    /// Data layers, in display order.
    pub layers: Vec<Layer>,
    /// Voice clips slot (crate `voice`, VO3).
    pub voice: VoiceSlot,
    /// Layers looked for but absent from the pipeline outputs, with the reason.
    #[serde(default)]
    pub missing: Vec<Missing>,
    /// Attributions to display, deduplicated, in layer order.
    pub attributions: Vec<String>,
    /// Every file of the pack except the manifest: size and SHA-256.
    pub files: Vec<FileEntry>,
    /// Sum of the sizes in [`Manifest::files`].
    pub total_bytes: u64,
}

/// Site description.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SiteInfo {
    /// Site identifier (registry identifier, e.g. `les-saisies`).
    pub id: String,
    /// Display name.
    pub name: String,
    /// Département number.
    pub department: String,
    /// WGS84 bounds `[west, south, east, north]`.
    pub bbox: [f64; 4],
    /// Commune centre `[lon, lat]`.
    pub center: [f64; 2],
    /// What the phone needs of the site to plan a day on it offline (P163, #163): kind,
    /// working box, massifs, the trail starts the pipeline found (X9) — the registry
    /// record of the site, without its status sources. `None` in packs written before it:
    /// the phone then knows only the evaluation sites.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub record: Option<SiteRecord>,
    /// How the planner ranked the starts of [`SiteInfo::record`] (#166): the starts of
    /// the record are in this order, best first. `None` in packs written before it, and
    /// for a site without starts in its record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_ranking: Option<StartRanking>,
}

/// The starts of a site ranked by the planner at the end of the pack build (#166): the
/// morning of the phone (`skimap_plan::plan::morning_in`, the same code) run on the pack
/// itself from each start, for the app's default outing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StartRanking {
    /// Time available asked, minutes (the app's default outing).
    pub budget_min: f64,
    /// Technique asked (`classic`, the app's default).
    pub style: String,
    /// Day asked (`AAAA-MM-JJ`): the first day of the pack's forecast, else the build day.
    pub date: String,
    /// Start time asked (`HH:MM`).
    pub start_time: String,
    /// Every start of the record, in its order (best first), with what the planner found.
    pub starts: Vec<StartScore>,
    /// Why the starts kept the registry order (the planner could not run), if so.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// Time of the ranking (view of the pack, layers, one morning per start), ms.
    pub compute_ms: u64,
}

/// What the planner found from one start ([`StartRanking`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StartScore {
    /// Identifier of the start (`skimap_common::site::Start::id`).
    pub id: String,
    /// Its place in the registry (0: the start the pipeline put first, X9).
    pub registry_rank: usize,
    /// Loops proposed (at most 3, the morning's), out-and-backs included.
    pub loops: usize,
    /// Of which real loops (less than a quarter of their length travelled twice).
    pub real_loops: usize,
    /// Score of the best loop (`skimap_assist::suggest`), if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub best_score: Option<f64>,
    /// Length of each loop, km.
    pub lengths_km: Vec<f64>,
    /// Length of each loop on stretches travelled once (its network km), km.
    pub network_km: Vec<f64>,
    /// The planner's last note when it found no loop.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// Validity of the whole pack.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PackValidity {
    /// Earliest `valid_until` date (`AAAA-MM-JJ`) found among the layers, if any.
    pub until: Option<String>,
    /// Layer that sets [`PackValidity::until`].
    pub until_layer: Option<String>,
    /// Human-readable note, French.
    pub note: String,
}

/// A source of a layer or basemap: the shared `<couche>.meta.json` schema
/// (`skimap_common::meta::SourceRef`), copied as written by the producing crate.
pub use skimap_common::meta::SourceRef as Source;

/// A basemap: vector tiles and a MapLibre style.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Basemap {
    /// Identifier (`plan-ign`, `osm`).
    pub id: String,
    /// Display title, French.
    pub title: String,
    /// PMTiles archive (vector tiles, MVT).
    pub tiles: String,
    /// MapLibre style; URLs inside are relative to the pack root
    /// (`pmtiles://basemap/x.pmtiles`, `basemap/x/fonts/{fontstack}/{range}.pbf`).
    pub style: String,
    /// Lowest zoom stored.
    pub minzoom: u8,
    /// Highest zoom stored; the map overzooms beyond.
    pub maxzoom: u8,
    /// Number of tiles stored.
    pub tile_count: u64,
    /// Size of the archive, style, sprites and fonts.
    pub bytes: u64,
    /// Date of the data (build date, server date).
    pub data_date: Option<String>,
    /// Sources.
    pub sources: Vec<Source>,
    /// Notes, French.
    #[serde(default)]
    pub notes: Vec<String>,
}

/// What kind of data a layer holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LayerKind {
    /// GeoJSON features, drawn by the viewer.
    Vector,
    /// Raster tiles (PNG in PMTiles), drawn by the viewer.
    Raster,
    /// Data for the app (JSON), not drawn.
    Data,
}

/// A data layer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Layer {
    /// Identifier, unique in the pack (`pistes`, `slope`, `zones`...).
    pub id: String,
    /// Display title, French.
    pub title: String,
    /// Kind of data.
    pub kind: LayerKind,
    /// `geojson`, `pmtiles` or `json`.
    pub format: String,
    /// Main file.
    pub file: String,
    /// Copy of the pipeline `<couche>.meta.json`, when there is one.
    #[serde(default)]
    pub meta: Option<String>,
    /// Pipeline layer directory it comes from (`data/out/<site>/<couche>/`).
    pub origin: String,
    /// Generation time of the source layer.
    #[serde(default)]
    pub generated_at: Option<String>,
    /// Start of validity, or date of the state described (`LayerMeta::valid_from`).
    #[serde(default)]
    pub valid_from: Option<String>,
    /// End of validity (`LayerMeta::valid_until`).
    #[serde(default)]
    pub valid_until: Option<String>,
    /// Validity in words (`LayerMeta::validity`).
    #[serde(default)]
    pub validity: Option<String>,
    /// Sources.
    pub sources: Vec<Source>,
    /// Whether any source is personal-use only.
    pub personal_use_only: bool,
    /// Size of [`Layer::file`].
    pub bytes: u64,
    /// Number of features (GeoJSON).
    #[serde(default)]
    pub features: Option<u64>,
    /// Geometry types present (GeoJSON).
    #[serde(default)]
    pub geometry_types: Vec<String>,
    /// Shown when the viewer opens.
    pub visible: bool,
    /// Raster description (raster layers).
    #[serde(default)]
    pub raster: Option<RasterInfo>,
    /// Class polygons in vector tiles (slope, avalanche exposure): zooms, legend, method
    /// and the areas measured against the source raster.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub classes: Option<ClassTilesInfo>,
    /// Notes, French (derived properties, limits).
    #[serde(default)]
    pub notes: Vec<String>,
}

/// How raster pixels map to values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValueEncoding {
    /// 8-bit palette PNG: the palette index is the class value
    /// ([`LegendEntry::value`]); index [`RasterInfo::nodata_index`] means no data.
    PaletteIndex,
    /// RGBA PNG as served by a third party: classes only through the legend colours.
    RgbaLegend,
}

/// Raster layer description.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RasterInfo {
    /// Lowest zoom stored.
    pub minzoom: u8,
    /// Highest zoom stored (values are read at this zoom).
    pub maxzoom: u8,
    /// Tile size in pixels.
    pub tile_size: u32,
    /// Number of tiles stored.
    pub tile_count: u64,
    /// Pixel encoding.
    pub encoding: ValueEncoding,
    /// Palette index of "no data" ([`ValueEncoding::PaletteIndex`]).
    #[serde(default)]
    pub nodata_index: Option<u8>,
    /// Classes.
    pub legend: Vec<LegendEntry>,
    /// Source raster: file, CRS and resolution.
    pub source: String,
    /// How lower zooms are made from the highest one.
    pub downsampling: String,
}

/// Class polygons of a raster of classes, in vector tiles (format `pmtiles`, MVT).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClassTilesInfo {
    /// Lowest zoom stored.
    pub minzoom: u8,
    /// Highest zoom stored; the map overzooms beyond.
    pub maxzoom: u8,
    /// Tile extent (4096).
    pub extent: u32,
    /// Number of tiles stored.
    pub tile_count: u64,
    /// Vector tile layer (`source-layer` of a style).
    pub source_layer: String,
    /// Attribute holding the class value ([`LegendEntry::value`]).
    pub property: String,
    /// Classes drawn, with their reference colour (light theme, export).
    pub legend: Vec<LegendEntry>,
    /// Source raster: file, CRS and resolution.
    pub source: String,
    /// Method, French.
    pub method: String,
    /// Patches and holes of the first level smaller than this were removed (m²).
    pub min_patch_m2: f64,
    /// Same for the higher levels (m²), whose removal leaves a drawn class.
    pub min_inner_patch_m2: f64,
    /// Area from which a raw zone counts as significant in [`LevelCheck`] (m²).
    pub significant_m2: f64,
    /// Simplification tolerance per zoom (m).
    pub tolerance_m: Vec<ZoomValue>,
    /// Polygons at the highest zoom.
    pub polygons: u64,
    /// Vertices per zoom (before the cut into tiles).
    pub vertices: Vec<ZoomValue>,
    /// Regions whose boundary gave more than one exterior ring (expected 0).
    pub regions_with_several_exteriors: u64,
    /// Areas per class.
    pub classes: Vec<ClassArea>,
    /// Areas and significant zones per threshold level (`class >= value`).
    pub levels: Vec<LevelCheck>,
}

/// A value at a zoom.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ZoomValue {
    /// Zoom.
    pub zoom: u8,
    /// Value.
    pub value: f64,
}

/// Area of one class: source raster, after smoothing, polygons per zoom (m², Lambert-93).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClassArea {
    /// Class value.
    pub value: u8,
    /// Label, French.
    pub label: String,
    /// Cells of the source raster in the class.
    pub raster_m2: f64,
    /// Cells in the class after smoothing and removal of small patches.
    pub filtered_m2: f64,
    /// Area of the polygons per zoom.
    pub vector_m2: Vec<ZoomValue>,
}

/// One threshold level (`class >= from_value`): what the smoothing removed and added,
/// and whether a significant zone of the source raster disappeared.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LevelCheck {
    /// Lowest class value of the level.
    pub from_value: u8,
    /// Label, French (« 30° et plus »).
    pub label: String,
    /// Source cells in the level (m²).
    pub raster_m2: f64,
    /// Source cells of the level that left it (m²).
    pub lost_m2: f64,
    /// Cells added to the level (m²).
    pub gained_m2: f64,
    /// Polygons of the level at the highest zoom (m²).
    pub vector_m2: f64,
    /// 4-connected zones of the source level of at least `significant_m2`.
    pub significant_zones: u64,
    /// Smallest share of such a zone still in the level after smoothing.
    pub significant_min_coverage: f64,
    /// Such zones keeping less than half of their cells.
    pub significant_below_half: u64,
    /// Largest source zone of the level removed entirely (m²).
    pub largest_removed_m2: f64,
}

/// One class of a raster legend.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LegendEntry {
    /// Class value (palette index).
    pub value: u8,
    /// Label, French.
    pub label: String,
    /// Colour `#rrggbbaa` (alpha 00: not drawn).
    pub color: String,
}

/// Voice clips slot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VoiceSlot {
    /// `present` or `absent`.
    pub status: String,
    /// Language of the clips (`fr`, `en`) in a voice pack apart from the base pack (VO6);
    /// `None` in a site pack (whose clips, if any, are French).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lang: Option<String>,
    /// Clip manifest of the voice crate (`voice/manifest.json`).
    #[serde(default)]
    pub manifest: Option<String>,
    /// Directory of the clips.
    #[serde(default)]
    pub clips_dir: Option<String>,
    /// Number of clip files.
    pub clips: u64,
    /// Size of the clips and their manifest.
    pub bytes: u64,
    /// Sources (TTS model, voice, names).
    #[serde(default)]
    pub sources: Vec<Source>,
    /// Note, French.
    pub note: String,
}

/// A layer that was looked for and not included.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Missing {
    /// Layer identifier.
    pub id: String,
    /// Why, French.
    pub reason: String,
}

/// A file of the pack.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileEntry {
    /// Path relative to the pack root.
    pub path: String,
    /// Size in bytes.
    pub bytes: u64,
    /// SHA-256, lowercase hex.
    pub sha256: String,
}

impl Manifest {
    /// Layer by identifier.
    pub fn layer(&self, id: &str) -> Option<&Layer> {
        self.layers.iter().find(|l| l.id == id)
    }

    /// Basemap by identifier.
    pub fn basemap(&self, id: &str) -> Option<&Basemap> {
        self.basemaps.iter().find(|b| b.id == id)
    }
}
