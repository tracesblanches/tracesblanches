//! Property tests of the phone-side pack reader helpers (Q3, #103): palette PNG round trip
//! and corruption handling, tile and pixel arithmetic.

use proptest::prelude::*;
use skimap_common::crs;
use skimap_common::geom::{BBox, LonLat};
use skimap_pack::png::{decode_indexed, encode_indexed};
use skimap_pack::tiles::{
    TILE_SIZE, global_pixel, pixel_lat, pixel_lon, tile_pixel, tiles_covering,
};

proptest! {
    /// Encode then decode gives the same image; any single corrupted byte or truncation is
    /// an error or a different valid image, never a panic.
    #[test]
    fn png_round_trip(
        (w, h) in (1u32..64, 1u32..64),
        palette in prop::collection::vec(any::<[u8; 4]>(), 1..=256),
        seed in any::<u64>(),
        flip in any::<prop::sample::Index>(),
        cut in any::<prop::sample::Index>(),
    ) {
        let n = palette.len() as u64;
        let mut x = seed | 1;
        let indices: Vec<u8> = (0..w * h)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                (x % n) as u8
            })
            .collect();
        let bytes = encode_indexed(w, h, &palette, &indices).expect("encode");
        let img = decode_indexed(&bytes).expect("decode");
        prop_assert_eq!(img.width, w);
        prop_assert_eq!(img.height, h);
        prop_assert_eq!(&img.indices, &indices);
        // tRNS covers the entries up to the last non-opaque one; the rest is opaque.
        for (a, b) in img.palette.iter().zip(&palette) {
            prop_assert_eq!(a, b);
        }
        prop_assert_eq!(img.index_at(w, 0), None);
        prop_assert_eq!(img.index_at(0, 0), Some(indices[0]));
        let mut bad = bytes.clone();
        let i = flip.index(bad.len());
        bad[i] ^= 0x5A;
        let _ = decode_indexed(&bad);
        let _ = decode_indexed(&bytes[..cut.index(bytes.len())]);
    }

    /// A position is inside the pixel the tile arithmetic gives it, at every zoom of the
    /// packs.
    #[test]
    fn pixel_contains_position(lon in -179.9f64..179.9, lat in -85.0f64..85.0, z in 0u8..=20) {
        let p = LonLat::new(lon, lat);
        let (tx, ty, px, py) = tile_pixel(p, z);
        prop_assert!(px < TILE_SIZE && py < TILE_SIZE);
        prop_assert_eq!((tx, ty), crs::tile_for(p, z));
        let gx = f64::from(tx * TILE_SIZE + px);
        let gy = f64::from(ty * TILE_SIZE + py);
        let eps = 1e-9;
        prop_assert!(pixel_lon(gx, z) <= lon + eps && lon <= pixel_lon(gx + 1.0, z) + eps);
        prop_assert!(pixel_lat(gy + 1.0, z) <= lat + eps && lat <= pixel_lat(gy, z) + eps);
        let (fx, fy) = global_pixel(p, z);
        prop_assert!((fx.floor() - gx).abs() < 1.0 + eps && (fy.floor() - gy).abs() < 1.0 + eps);
    }

    /// The tiles covering a box contain the tile of every point of the box, once.
    #[test]
    fn tiles_cover_the_box(
        (s, w) in (44.0f64..47.0, 5.0f64..7.0),
        (dn, de) in (0.001f64..0.3, 0.001f64..0.3),
        (u, v) in (0.0f64..=1.0, 0.0f64..=1.0),
        (z0, dz) in (8u8..14, 0u8..3),
    ) {
        let b = BBox::new(s, w, s + dn, w + de);
        let tiles = tiles_covering(b, z0, z0 + dz);
        let p = LonLat::new(w + u * de, s + v * dn);
        for z in z0..=z0 + dz {
            let (x, y) = crs::tile_for(p, z);
            prop_assert_eq!(tiles.iter().filter(|t| **t == (z, x, y)).count(), 1);
        }
    }
}
