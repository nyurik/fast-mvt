//! Shared decode benchmark logic — the *same* measured work for both harnesses: the wall-clock
//! (criterion) `decoder` bench and the CPU-instruction-count (gungraun/Valgrind) `decoder_cpu` bench.
//! Only the harness wrapper differs; this module is harness-agnostic.
//!
//! Every [`Dec`] variant traverses each fixture tile once (id, geometry, and all properties).

#![allow(
    dead_code,
    reason = "each bench binary uses only the parts of `common` it needs"
)]

use std::hint::black_box;

use fast_mvt::MvtReaderRef;
use prost::Message as _;
use tinymvt::geometry::GeometryDecoder as TinyGeometryDecoder;
use tinymvt::tag::TagsDecoder as TinyTagsDecoder;
use tinymvt::vector_tile::{Tile as TinyTile, tile as tiny_tile};

use super::BenchTile;

/// Which decoder a benchmark case exercises.
#[derive(Clone, Copy)]
pub enum Dec {
    /// [`fast_mvt`]'s zero-copy [`MvtReaderRef`] traversal.
    Fast,
    /// The [`mvt_reader`] crate.
    MvtReader,
    /// [`tinymvt`] (prost-decoded).
    Tiny,
}

impl Dec {
    /// Every variant, in a stable order (used by the criterion harness to build its comparison group).
    pub const ALL: [Self; 3] = [Self::Fast, Self::MvtReader, Self::Tiny];

    /// Human-readable label (the criterion benchmark name; matches the pre-gungraun output).
    pub fn label(self) -> &'static str {
        match self {
            Self::Fast => "fast-mvt traverse",
            Self::MvtReader => "mvt-reader traverse",
            Self::Tiny => "tinymvt traverse",
        }
    }
}

/// Traverse every tile once with the selected decoder — the measured region, shared by both harnesses.
pub fn run(dec: Dec, tiles: &[BenchTile]) {
    for tile in tiles {
        let data = black_box(tile.data.as_slice());
        match dec {
            Dec::Fast => traverse_fast_mvt(data),
            Dec::MvtReader => traverse_mvt_reader(data),
            Dec::Tiny => traverse_tinymvt(data),
        }
    }
}

fn traverse_fast_mvt(data: &[u8]) {
    let reader = MvtReaderRef::new(data).expect("fast-mvt parse");
    for layer in reader.layers() {
        for feature in layer.features() {
            black_box(feature.id());
            black_box(feature.geometry().expect("fast-mvt geometry"));
            for property in feature.properties() {
                black_box(property.expect("fast-mvt property"));
            }
        }
    }
}

fn traverse_mvt_reader(data: &[u8]) {
    let reader = mvt_reader::Reader::new(data.to_vec()).expect("mvt-reader parse");

    for layer in reader
        .get_layer_metadata()
        .expect("mvt-reader layer metadata")
    {
        for feature in reader
            .get_features_as::<i32>(layer.layer_index)
            .expect("mvt-reader features")
        {
            black_box(feature.id);
            black_box(feature.get_geometry());
            for property in feature.properties.as_ref().expect("mvt-reader properties") {
                black_box(property);
            }
        }
    }
}

fn traverse_tinymvt(data: &[u8]) {
    let tile = TinyTile::decode(data).expect("tinymvt parse");
    for layer in &tile.layers {
        let tags = TinyTagsDecoder::new(&layer.keys, &layer.values);
        for feature in &layer.features {
            black_box(feature.id);
            black_box(tags.decode(&feature.tags).expect("tinymvt tags"));
            decode_tiny_geometry(feature).expect("tinymvt geometry");
        }
    }
}

fn decode_tiny_geometry(feature: &tiny_tile::Feature) -> Result<(), String> {
    let mut geometry = TinyGeometryDecoder::new(&feature.geometry);
    match feature
        .r#type
        .and_then(|value| tiny_tile::GeomType::try_from(value).ok())
    {
        Some(tiny_tile::GeomType::Point) => {
            black_box(geometry.decode_points()?);
        }
        Some(tiny_tile::GeomType::Linestring) => {
            black_box(geometry.decode_linestrings()?);
        }
        Some(tiny_tile::GeomType::Polygon) => {
            black_box(geometry.decode_polygons()?);
        }
        Some(tiny_tile::GeomType::Unknown) | None => {}
    }
    Ok(())
}
