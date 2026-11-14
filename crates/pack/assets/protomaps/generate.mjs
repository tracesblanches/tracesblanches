// Generates the MapLibre style of the OSM (Protomaps) basemap of the packs.
//
// Tooling only, run once per @protomaps/basemaps upgrade; the output is committed:
//   mkdir /tmp/pm && cd /tmp/pm && npm init -y && npm install @protomaps/basemaps@5.7.2
//   node <repo>/crates/pack/assets/protomaps/generate.mjs <repo>/crates/pack/assets/protomaps/style-light-fr.json
//
// URLs in the output are relative to the pack root; the viewer (or the app) makes them
// absolute.
import { createRequire } from "node:module";
import { writeFileSync } from "node:fs";

const require = createRequire(process.cwd() + "/");
const basemaps = require("@protomaps/basemaps");
const pkg = require("@protomaps/basemaps/package.json");

const out = process.argv[2];
if (!out) throw new Error("usage: generate.mjs <output.json>");

const style = {
  version: 8,
  name: `Protomaps light (fr), @protomaps/basemaps ${pkg.version}`,
  metadata: {
    "skimap:relative_urls": true,
    "skimap:generator": `@protomaps/basemaps ${pkg.version}`,
  },
  glyphs: "basemap/osm/fonts/{fontstack}/{range}.pbf",
  sprite: "basemap/osm/sprites/light",
  sources: {
    protomaps: {
      type: "vector",
      url: "pmtiles://basemap/osm.pmtiles",
      attribution:
        '<a href="https://protomaps.com">Protomaps</a> © <a href="https://openstreetmap.org/copyright">OpenStreetMap</a>',
    },
  },
  layers: basemaps.layers("protomaps", basemaps.namedFlavor("light"), { lang: "fr" }),
};
writeFileSync(out, JSON.stringify(style, null, 1) + "\n");
console.log(`${style.layers.length} layers written to ${out}`);
