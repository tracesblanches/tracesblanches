//! Streaming reader of an OSM XML file (optionally gzip-compressed), keeping only what lies
//! in an area of interest.
//!
//! Built for OpenSnowMap's `planet_pistes.osm.gz` (≈ 200 MB compressed, 1.8 GB of XML,
//! 8.7 M nodes, 480 k ways, 17 k relations, written by osmium: nodes, then ways, then
//! relations). The file is read once; decompression runs on its own thread so that it
//! overlaps with XML parsing.
//!
//! Filtering:
//! - a node is kept if it lies in one of the area-of-interest boxes;
//! - a way is kept if at least one of its nodes was kept (its node list is kept whole;
//!   references to dropped nodes are skipped when building geometries);
//! - a relation is kept if it has, directly or through nested relations, a kept way.

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;
use std::sync::mpsc;

use quick_xml::XmlVersion;
use quick_xml::events::{BytesStart, Event};
use skimap_common::geom::{BBox, LonLat};

use crate::error::{OsmError, Result};

/// OSM tags of an element, in file order.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Tags(pub Vec<(String, String)>);

impl Tags {
    /// Value of `key`, if present.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    /// Value of `key`, if present and not blank.
    pub fn non_empty(&self, key: &str) -> Option<&str> {
        self.get(key).map(str::trim).filter(|v| !v.is_empty())
    }

    /// Whether `key` has the value `value`.
    pub fn is(&self, key: &str, value: &str) -> bool {
        self.get(key) == Some(value)
    }

    /// Whether the `;`-separated list in `key` contains `value`.
    pub fn list_contains(&self, key: &str, value: &str) -> bool {
        self.get(key)
            .is_some_and(|v| v.split(';').any(|p| p.trim() == value))
    }
}

/// Kind of a relation member.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MemberKind {
    /// A node.
    Node,
    /// A way.
    Way,
    /// A relation.
    Relation,
}

/// Member of a relation.
#[derive(Debug, Clone, PartialEq)]
pub struct Member {
    /// Kind of the member.
    pub kind: MemberKind,
    /// OSM id of the member.
    pub id: i64,
    /// Role, possibly empty.
    pub role: String,
}

/// An OSM way.
#[derive(Debug, Clone, PartialEq)]
pub struct Way {
    /// OSM id.
    pub id: i64,
    /// Node references, in order.
    pub nodes: Vec<i64>,
    /// Tags.
    pub tags: Tags,
}

/// An OSM relation.
#[derive(Debug, Clone, PartialEq)]
pub struct Relation {
    /// OSM id.
    pub id: i64,
    /// Members, in order.
    pub members: Vec<Member>,
    /// Tags.
    pub tags: Tags,
}

/// Parsed OSM data restricted to an area of interest.
#[derive(Debug, Default)]
pub struct OsmData {
    node_ids: Vec<i64>,
    node_pos: Vec<[i32; 2]>,
    /// Kept ways, in file order.
    pub ways: Vec<Way>,
    /// Kept relations, in file order.
    pub relations: Vec<Relation>,
    way_index: HashMap<i64, usize>,
    relation_index: HashMap<i64, usize>,
    /// Statistics of the read.
    pub stats: ReadStats,
}

/// Counts gathered while reading.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ReadStats {
    /// Nodes in the file.
    pub nodes_read: u64,
    /// Ways in the file.
    pub ways_read: u64,
    /// Relations in the file.
    pub relations_read: u64,
    /// Uncompressed bytes read.
    pub bytes: u64,
}

const E7: f64 = 1e7;

impl OsmData {
    /// Builds data from elements (tests and small inputs). Nodes are given with their
    /// positions; every way and relation is kept.
    pub fn from_elements(
        nodes: impl IntoIterator<Item = (i64, LonLat)>,
        ways: Vec<Way>,
        relations: Vec<Relation>,
    ) -> Self {
        let mut n: Vec<(i64, [i32; 2])> = nodes
            .into_iter()
            .map(|(id, p)| (id, [to_e7(p.lon), to_e7(p.lat)]))
            .collect();
        n.sort_unstable_by_key(|(id, _)| *id);
        let mut d = OsmData {
            node_ids: n.iter().map(|(id, _)| *id).collect(),
            node_pos: n.iter().map(|(_, p)| *p).collect(),
            ways,
            relations,
            ..Default::default()
        };
        d.reindex();
        d
    }

    fn reindex(&mut self) {
        self.way_index = self
            .ways
            .iter()
            .enumerate()
            .map(|(i, w)| (w.id, i))
            .collect();
        self.relation_index = self
            .relations
            .iter()
            .enumerate()
            .map(|(i, r)| (r.id, i))
            .collect();
    }

    /// Position of a kept node.
    pub fn node(&self, id: i64) -> Option<LonLat> {
        let i = self.node_ids.binary_search(&id).ok()?;
        let [lon, lat] = self.node_pos[i];
        Some(LonLat::new(lon as f64 / E7, lat as f64 / E7))
    }

    /// Number of kept nodes.
    pub fn node_count(&self) -> usize {
        self.node_ids.len()
    }

    /// Way by id.
    pub fn way(&self, id: i64) -> Option<&Way> {
        self.way_index.get(&id).map(|&i| &self.ways[i])
    }

    /// Relation by id.
    pub fn relation(&self, id: i64) -> Option<&Relation> {
        self.relation_index.get(&id).map(|&i| &self.relations[i])
    }

    /// Geometry of a way: positions of its kept nodes, with their ids.
    pub fn way_points(&self, w: &Way) -> Vec<(i64, LonLat)> {
        w.nodes
            .iter()
            .filter_map(|&n| self.node(n).map(|p| (n, p)))
            .collect()
    }
}

fn to_e7(deg: f64) -> i32 {
    (deg * E7).round() as i32
}

/// Area of interest: union of boxes.
#[derive(Debug, Clone)]
pub struct Aoi(pub Vec<BBox>);

impl Aoi {
    fn contains(&self, p: LonLat) -> bool {
        self.0.iter().any(|b| b.contains(p))
    }
}

/// Reads an OSM XML file, gzip-compressed if its name ends in `.gz` or its content starts
/// with the gzip magic number.
pub fn read_file(path: &Path, aoi: &Aoi) -> Result<OsmData> {
    let mut f = File::open(path).map_err(|e| OsmError::io(path, e))?;
    let mut magic = [0u8; 2];
    let gz = f.read_exact(&mut magic).is_ok() && magic == [0x1f, 0x8b];
    drop(f);
    let f = File::open(path).map_err(|e| OsmError::io(path, e))?;
    if gz {
        // Decompress on a separate thread; chunks travel through a bounded channel.
        let (tx, rx) = mpsc::sync_channel::<std::io::Result<Vec<u8>>>(16);
        let path_buf = path.to_path_buf();
        std::thread::scope(|s| {
            s.spawn(move || {
                let mut dec =
                    flate2::read::MultiGzDecoder::new(BufReader::with_capacity(1 << 20, f));
                loop {
                    let mut chunk = vec![0u8; 1 << 20];
                    match read_full(&mut dec, &mut chunk) {
                        Ok(0) => break,
                        Ok(n) => {
                            chunk.truncate(n);
                            if tx.send(Ok(chunk)).is_err() {
                                break;
                            }
                        }
                        Err(e) => {
                            let _ = tx.send(Err(e));
                            break;
                        }
                    }
                }
            });
            let reader = BufReader::with_capacity(1 << 20, ChannelReader::new(rx));
            read(reader, aoi).map_err(|e| match e {
                OsmError::Xml { message, .. } => OsmError::Xml {
                    path: path_buf.display().to_string(),
                    message,
                },
                other => other,
            })
        })
    } else {
        read(BufReader::with_capacity(1 << 20, f), aoi)
    }
}

fn read_full(r: &mut impl Read, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut n = 0;
    while n < buf.len() {
        match r.read(&mut buf[n..]) {
            Ok(0) => break,
            Ok(k) => n += k,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(n)
}

/// `Read` over chunks received from a channel.
struct ChannelReader {
    rx: mpsc::Receiver<std::io::Result<Vec<u8>>>,
    cur: Vec<u8>,
    pos: usize,
}

impl ChannelReader {
    fn new(rx: mpsc::Receiver<std::io::Result<Vec<u8>>>) -> Self {
        Self {
            rx,
            cur: Vec::new(),
            pos: 0,
        }
    }
}

impl Read for ChannelReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        while self.pos >= self.cur.len() {
            match self.rx.recv() {
                Ok(Ok(c)) => {
                    self.cur = c;
                    self.pos = 0;
                }
                Ok(Err(e)) => return Err(e),
                Err(_) => return Ok(0),
            }
        }
        let n = buf.len().min(self.cur.len() - self.pos);
        buf[..n].copy_from_slice(&self.cur[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
}

enum Current {
    None,
    Way(Way),
    Relation(Relation),
}

/// Reads OSM XML from any buffered reader.
pub fn read<R: BufRead>(input: R, aoi: &Aoi) -> Result<OsmData> {
    let mut reader = quick_xml::Reader::from_reader(input);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::with_capacity(4096);
    let mut nodes: Vec<(i64, [i32; 2])> = Vec::new();
    let mut ways = Vec::new();
    let mut relations = Vec::new();
    let mut stats = ReadStats::default();
    let mut cur = Current::None;
    let mut nodes_sorted = false;

    let xml_err = |reader: &quick_xml::Reader<R>, e: String| OsmError::Xml {
        path: String::new(),
        message: format!("at byte {}: {e}", reader.buffer_position()),
    };

    loop {
        let ev = reader
            .read_event_into(&mut buf)
            .map_err(|e| xml_err(&reader, e.to_string()))?;
        match ev {
            Event::Eof => break,
            Event::Start(ref e) | Event::Empty(ref e) => {
                let empty = matches!(ev, Event::Empty(_));
                match e.name().as_ref() {
                    "node" => {
                        stats.nodes_read += 1;
                        let (id, lon, lat) = node_attrs(e).map_err(|m| xml_err(&reader, m))?;
                        let p = LonLat::new(lon, lat);
                        if aoi.contains(p) {
                            nodes.push((id, [to_e7(lon), to_e7(lat)]));
                        }
                        // Node tags are not needed: children are skipped as unknown elements.
                    }
                    "way" => {
                        stats.ways_read += 1;
                        let id = id_attr(e).map_err(|m| xml_err(&reader, m))?;
                        let w = Way {
                            id,
                            nodes: Vec::new(),
                            tags: Tags::default(),
                        };
                        if empty {
                            // A way without nodes is never kept.
                        } else {
                            cur = Current::Way(w);
                        }
                    }
                    "relation" => {
                        stats.relations_read += 1;
                        let id = id_attr(e).map_err(|m| xml_err(&reader, m))?;
                        let r = Relation {
                            id,
                            members: Vec::new(),
                            tags: Tags::default(),
                        };
                        if empty {
                            relations.push(r);
                        } else {
                            cur = Current::Relation(r);
                        }
                    }
                    "nd" => {
                        if let Current::Way(w) = &mut cur {
                            let r = attr_i64(e, "ref").map_err(|m| xml_err(&reader, m))?;
                            w.nodes.push(r);
                        }
                    }
                    "tag" => {
                        let (k, v) = tag_attrs(e).map_err(|m| xml_err(&reader, m))?;
                        match &mut cur {
                            Current::Way(w) => w.tags.0.push((k, v)),
                            Current::Relation(r) => r.tags.0.push((k, v)),
                            Current::None => {}
                        }
                    }
                    "member" => {
                        if let Current::Relation(r) = &mut cur {
                            r.members
                                .push(member_attrs(e).map_err(|m| xml_err(&reader, m))?);
                        }
                    }
                    _ => {}
                }
            }
            Event::End(ref e) => match e.name().as_ref() {
                "way" => {
                    if let Current::Way(w) = std::mem::replace(&mut cur, Current::None) {
                        if !nodes_sorted {
                            // First way: nodes are complete. osmium writes them sorted, but
                            // do not rely on it.
                            nodes.sort_unstable_by_key(|(id, _)| *id);
                            nodes.dedup_by_key(|(id, _)| *id);
                            nodes_sorted = true;
                        }
                        if w.nodes
                            .iter()
                            .any(|n| nodes.binary_search_by_key(n, |(id, _)| *id).is_ok())
                        {
                            ways.push(w);
                        }
                    }
                }
                "relation" => {
                    if let Current::Relation(r) = std::mem::replace(&mut cur, Current::None) {
                        relations.push(r);
                    }
                }
                _ => {}
            },
            _ => {}
        }
        buf.clear();
    }
    stats.bytes = reader.buffer_position();
    if !nodes_sorted {
        nodes.sort_unstable_by_key(|(id, _)| *id);
        nodes.dedup_by_key(|(id, _)| *id);
    }

    // Keep relations reaching a kept way, directly or through nested relations.
    let way_ids: HashSet<i64> = ways.iter().map(|w| w.id).collect();
    let by_id: HashMap<i64, usize> = relations
        .iter()
        .enumerate()
        .map(|(i, r)| (r.id, i))
        .collect();
    let mut keep = vec![false; relations.len()];
    for (i, r) in relations.iter().enumerate() {
        keep[i] = r
            .members
            .iter()
            .any(|m| m.kind == MemberKind::Way && way_ids.contains(&m.id));
    }
    // Propagate upwards until stable (nesting depth is small in practice).
    loop {
        let mut changed = false;
        for (i, r) in relations.iter().enumerate() {
            if keep[i] {
                continue;
            }
            if r.members.iter().any(|m| {
                m.kind == MemberKind::Relation && by_id.get(&m.id).is_some_and(|&j| keep[j])
            }) {
                keep[i] = true;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    let relations: Vec<Relation> = relations
        .into_iter()
        .zip(keep)
        .filter_map(|(r, k)| k.then_some(r))
        .collect();

    let mut d = OsmData {
        node_ids: nodes.iter().map(|(id, _)| *id).collect(),
        node_pos: nodes.iter().map(|(_, p)| *p).collect(),
        ways,
        relations,
        stats,
        ..Default::default()
    };
    d.reindex();
    Ok(d)
}

fn attr_str(e: &BytesStart<'_>, name: &str) -> std::result::Result<Option<String>, String> {
    for a in e.attributes().with_checks(false) {
        let a = a.map_err(|e| e.to_string())?;
        if a.key.as_ref() == name {
            return a
                .normalized_value(XmlVersion::Implicit1_0)
                .map(|v| Some(v.into_owned()))
                .map_err(|e| e.to_string());
        }
    }
    Ok(None)
}

fn parse_raw<T: std::str::FromStr>(v: &str, what: &str) -> std::result::Result<T, String> {
    v.parse().map_err(|_| format!("invalid {what} `{v}`"))
}

fn attr_i64(e: &BytesStart<'_>, name: &str) -> std::result::Result<i64, String> {
    for a in e.attributes().with_checks(false) {
        let a = a.map_err(|e| e.to_string())?;
        if a.key.as_ref() == name {
            return parse_raw(&a.value, "id");
        }
    }
    Err(format!("missing attribute {name}"))
}

fn id_attr(e: &BytesStart<'_>) -> std::result::Result<i64, String> {
    attr_i64(e, "id")
}

fn node_attrs(e: &BytesStart<'_>) -> std::result::Result<(i64, f64, f64), String> {
    let (mut id, mut lon, mut lat) = (None, None, None);
    for a in e.attributes().with_checks(false) {
        let a = a.map_err(|e| e.to_string())?;
        match a.key.as_ref() {
            "id" => id = Some(parse_raw::<i64>(&a.value, "node id")?),
            "lon" => lon = Some(parse_raw::<f64>(&a.value, "lon")?),
            "lat" => lat = Some(parse_raw::<f64>(&a.value, "lat")?),
            _ => {}
        }
    }
    match (id, lon, lat) {
        (Some(i), Some(lo), Some(la)) => Ok((i, lo, la)),
        _ => Err("node without id, lat or lon".to_owned()),
    }
}

fn tag_attrs(e: &BytesStart<'_>) -> std::result::Result<(String, String), String> {
    let k = attr_str(e, "k")?.ok_or("tag without k")?;
    let v = attr_str(e, "v")?.unwrap_or_default();
    Ok((k, v))
}

fn member_attrs(e: &BytesStart<'_>) -> std::result::Result<Member, String> {
    let kind = match attr_str(e, "type")?.as_deref() {
        Some("node") => MemberKind::Node,
        Some("way") => MemberKind::Way,
        Some("relation") => MemberKind::Relation,
        other => return Err(format!("invalid member type {other:?}")),
    };
    let id = attr_i64(e, "ref")?;
    let role = attr_str(e, "role")?.unwrap_or_default();
    Ok(Member { kind, id, role })
}

#[cfg(test)]
mod tests {
    use super::*;

    const XML: &str = r#"<?xml version='1.0' encoding='UTF-8'?>
<osm version="0.6" generator="test">
  <node id="1" lat="46.60" lon="6.10"/>
  <node id="2" lat="46.61" lon="6.11">
    <tag k="tourism" v="information"/>
  </node>
  <node id="3" lat="10.0" lon="10.0"/>
  <node id="4" lat="10.1" lon="10.1"/>
  <way id="10">
    <nd ref="1"/><nd ref="2"/><nd ref="3"/>
    <tag k="piste:type" v="nordic"/>
    <tag k="name" v="L&apos;Aigalan &amp; co"/>
  </way>
  <way id="11">
    <nd ref="3"/><nd ref="4"/>
    <tag k="piste:type" v="nordic"/>
  </way>
  <relation id="100">
    <member type="way" ref="10" role=""/>
    <member type="way" ref="11" role="forward"/>
    <tag k="route" v="piste"/>
    <tag k="colour" v="red"/>
  </relation>
  <relation id="101">
    <member type="way" ref="11" role=""/>
  </relation>
  <relation id="102">
    <member type="relation" ref="100" role=""/>
    <tag k="site" v="piste"/>
  </relation>
</osm>"#;

    #[test]
    fn filters_by_area_and_keeps_relations_reaching_kept_ways() {
        let aoi = Aoi(vec![BBox::new(46.0, 6.0, 47.0, 7.0)]);
        let d = read(XML.as_bytes(), &aoi).unwrap();
        assert_eq!(d.stats.nodes_read, 4);
        assert_eq!(d.stats.ways_read, 2);
        assert_eq!(d.stats.relations_read, 3);
        assert_eq!(d.node_count(), 2);
        assert_eq!(d.ways.len(), 1);
        let w = d.way(10).unwrap();
        assert_eq!(w.nodes, vec![1, 2, 3]);
        assert_eq!(w.tags.get("name"), Some("L'Aigalan & co"));
        assert!(w.tags.list_contains("piste:type", "nordic"));
        // Node 3 is outside the area: skipped in the geometry.
        assert_eq!(d.way_points(w).len(), 2);
        let ids: Vec<i64> = d.relations.iter().map(|r| r.id).collect();
        assert_eq!(ids, vec![100, 102]);
        let r = d.relation(100).unwrap();
        assert_eq!(r.members[1].role, "forward");
        assert_eq!(r.members[1].kind, MemberKind::Way);
        let p = d.node(2).unwrap();
        assert!((p.lat - 46.61).abs() < 1e-9 && (p.lon - 6.11).abs() < 1e-9);
    }

    #[test]
    fn gzip_file_round_trip() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.osm.gz");
        let mut enc = flate2::write::GzEncoder::new(
            File::create(&path).unwrap(),
            flate2::Compression::fast(),
        );
        enc.write_all(XML.as_bytes()).unwrap();
        enc.finish().unwrap();
        let aoi = Aoi(vec![BBox::new(-90.0, -180.0, 90.0, 180.0)]);
        let d = read_file(&path, &aoi).unwrap();
        assert_eq!(d.node_count(), 4);
        assert_eq!(d.ways.len(), 2);
        assert_eq!(d.relations.len(), 3);
    }

    #[test]
    fn malformed_xml_is_an_error() {
        let aoi = Aoi(vec![BBox::new(-90.0, -180.0, 90.0, 180.0)]);
        let bad = r#"<osm><node id="x" lat="1" lon="2"/></osm>"#;
        assert!(read(bad.as_bytes(), &aoi).is_err());
    }
}
