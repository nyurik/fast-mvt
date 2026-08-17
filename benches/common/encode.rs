//! Shared encode benchmark logic — the *same* measured work for both harnesses: the wall-clock
//! (criterion) `encoder` bench and the CPU-instruction-count (gungraun/Valgrind) `encoder_cpu` bench.
//! Only the harness wrapper differs; this module is harness-agnostic.
//!
//! Every [`Enc`] variant encodes each fixture tile once. The `*_owned` variants include the input
//! clone in the measured work, matching how a caller that owns its geometry would pay for it.

#![allow(
    dead_code,
    reason = "each bench binary uses only the parts of `common` it needs"
)]

use std::hint::black_box;

use fast_mvt::{
    DEFAULT_EXTENT, MvtCoord, MvtGeometry, MvtLineString, MvtPolygon, MvtTile, MvtValue,
};
use geo_types::Geometry;
use mvt::{GeomEncoder, GeomType};
use prost::Message as _;
use tinymvt::geometry::GeometryEncoder as TinyGeometryEncoder;
use tinymvt::tag::{TagsEncoder as TinyTagsEncoder, Value as TinyValue};
use tinymvt::vector_tile::{Tile as TinyTile, tile as tiny_tile};

use super::BenchTile;

/// Which encoder a benchmark case exercises.
#[derive(Clone, Copy)]
pub enum Enc {
    /// [`fast_mvt`] consuming an owned tile (clone included in the measured work).
    FastOwned,
    /// [`fast_mvt`] borrowing the tile ([`MvtTile::encode_ref`]).
    Fast,
    /// The [`mvt`] crate, cloning the tile first.
    MvtOwned,
    /// The [`mvt`] crate, borrowing the tile.
    Mvt,
    /// [`tinymvt`], cloning the tile first.
    TinyOwned,
    /// [`tinymvt`], borrowing the tile.
    Tiny,
}

impl Enc {
    /// Every variant, in a stable order (used by the criterion harness to build its comparison group).
    pub const ALL: [Self; 6] = [
        Self::FastOwned,
        Self::Fast,
        Self::MvtOwned,
        Self::Mvt,
        Self::TinyOwned,
        Self::Tiny,
    ];

    /// Human-readable label (the criterion benchmark name; matches the pre-gungraun output).
    pub fn label(self) -> &'static str {
        match self {
            Self::FastOwned => "fast-mvt owned encode",
            Self::Fast => "fast-mvt encode",
            Self::MvtOwned => "mvt owned encode",
            Self::Mvt => "mvt encode",
            Self::TinyOwned => "tinymvt owned encode",
            Self::Tiny => "tinymvt encode",
        }
    }
}

/// Encode every tile once with the selected encoder — the measured region, shared by both harnesses.
pub fn run(enc: Enc, tiles: &[BenchTile]) {
    for tile in tiles {
        let bytes = match enc {
            Enc::FastOwned => tile.parsed.clone().encode().expect("fast-mvt encode"),
            Enc::Fast => tile.parsed.encode_ref().expect("fast-mvt encode"),
            Enc::MvtOwned => encode_with_mvt(&black_box(tile.parsed.clone())).expect("mvt encode"),
            Enc::Mvt => encode_with_mvt(&tile.parsed).expect("mvt encode"),
            Enc::TinyOwned => {
                encode_with_tinymvt(&black_box(tile.parsed.clone())).expect("tinymvt encode")
            }
            Enc::Tiny => encode_with_tinymvt(&tile.parsed).expect("tinymvt encode"),
        };
        black_box(bytes);
    }
}

fn encode_with_tinymvt(tile: &MvtTile) -> Result<Vec<u8>, String> {
    let mut out = TinyTile {
        layers: Vec::with_capacity(tile.layers.len()),
    };
    for layer in &tile.layers {
        let mut tags = TinyTagsEncoder::new();
        let mut features = Vec::with_capacity(layer.features.len());
        for feature in &layer.features {
            for (key, value) in &feature.properties {
                if let Some(value) = tiny_value(value) {
                    tags.add(key, value);
                }
            }
            let (geom_type, geometry) = tiny_geometry(&feature.geometry)?;
            features.push(tiny_tile::Feature {
                id: feature.id,
                tags: tags.take_tags(),
                r#type: Some(geom_type as i32),
                geometry,
            });
        }
        let (keys, values) = tags.into_keys_and_values();
        out.layers.push(tiny_tile::Layer {
            version: 2,
            name: layer.name.clone(),
            features,
            keys,
            values,
            extent: Some(layer.extent.get()),
        });
    }
    Ok(out.encode_to_vec())
}

fn tiny_value(value: &MvtValue) -> Option<TinyValue> {
    match value {
        MvtValue::String(value) => Some(TinyValue::String(value.clone())),
        MvtValue::Float(value) => Some(TinyValue::Float(value.to_ne_bytes())),
        MvtValue::Double(value) => Some(TinyValue::Double(value.to_ne_bytes())),
        MvtValue::Int(value) => Some(TinyValue::Int(*value)),
        MvtValue::UInt(value) => Some(TinyValue::Uint(*value)),
        MvtValue::SInt(value) => Some(TinyValue::SInt(*value)),
        MvtValue::Bool(value) => Some(TinyValue::Bool(*value)),
        MvtValue::Null => None,
    }
}

fn tiny_geometry(geometry: &MvtGeometry) -> Result<(tiny_tile::GeomType, Vec<u32>), String> {
    let mut encoder = TinyGeometryEncoder::new();
    let geom_type = match geometry {
        Geometry::Point(point) => {
            encoder.add_points([coord_array(point.0)]);
            tiny_tile::GeomType::Point
        }
        Geometry::MultiPoint(points) => {
            encoder.add_points(points.0.iter().map(|point| coord_array(point.0)));
            tiny_tile::GeomType::Point
        }
        Geometry::LineString(line) => {
            encoder.add_linestring(
                without_trailing_duplicate(&line.0)
                    .iter()
                    .map(|&coord| coord_array(coord)),
            );
            tiny_tile::GeomType::Linestring
        }
        Geometry::MultiLineString(lines) => {
            for line in &lines.0 {
                encoder.add_linestring(
                    without_trailing_duplicate(&line.0)
                        .iter()
                        .map(|&coord| coord_array(coord)),
                );
            }
            tiny_tile::GeomType::Linestring
        }
        Geometry::Polygon(polygon) => {
            add_tiny_polygon(&mut encoder, polygon);
            tiny_tile::GeomType::Polygon
        }
        Geometry::MultiPolygon(polygons) => {
            for polygon in &polygons.0 {
                add_tiny_polygon(&mut encoder, polygon);
            }
            tiny_tile::GeomType::Polygon
        }
        Geometry::GeometryCollection(collection) if collection.0.len() == 1 => {
            return tiny_geometry(&collection.0[0]);
        }
        _ => return Err("unsupported geometry".into()),
    };
    Ok((geom_type, encoder.into_vec()))
}

fn add_tiny_polygon(encoder: &mut TinyGeometryEncoder, polygon: &MvtPolygon) {
    encoder.add_ring(
        without_trailing_duplicate(&polygon.exterior().0)
            .iter()
            .map(|&coord| coord_array(coord)),
    );
    for ring in polygon.interiors() {
        encoder.add_ring(
            without_trailing_duplicate(&ring.0)
                .iter()
                .map(|&coord| coord_array(coord)),
        );
    }
}

fn coord_array(coord: MvtCoord) -> [i32; 2] {
    [coord.x, coord.y]
}

fn encode_with_mvt(tile: &MvtTile) -> Result<Vec<u8>, mvt::Error> {
    let extent = tile
        .layers
        .first()
        .map_or(DEFAULT_EXTENT, |v| v.extent)
        .get();
    let mut out = mvt::Tile::new(extent);
    for layer in &tile.layers {
        let mut mvt_layer = out.create_layer(&layer.name);
        for feature in &layer.features {
            let geom_data = mvt_geom_data(&feature.geometry)?;
            let mut mvt_feature = mvt_layer.into_feature(geom_data);
            if let Some(id) = feature.id {
                mvt_feature.set_id(id);
            }
            for (key, value) in &feature.properties {
                add_mvt_tag(&mut mvt_feature, key, value);
            }
            mvt_layer = mvt_feature.into_layer();
        }
        out.add_layer(mvt_layer)?;
    }
    out.to_bytes()
}

fn add_mvt_tag(feature: &mut mvt::Feature, key: &str, value: &MvtValue) {
    match value {
        MvtValue::String(value) => feature.add_tag_string(key, value),
        MvtValue::Float(value) => feature.add_tag_float(key, *value),
        MvtValue::Double(value) => feature.add_tag_double(key, *value),
        MvtValue::Int(value) => feature.add_tag_int(key, *value),
        MvtValue::UInt(value) => feature.add_tag_uint(key, *value),
        MvtValue::SInt(value) => feature.add_tag_sint(key, *value),
        MvtValue::Bool(value) => feature.add_tag_bool(key, *value),
        MvtValue::Null => {}
    }
}

fn mvt_geom_data(geometry: &MvtGeometry) -> Result<mvt::GeomData, mvt::Error> {
    let mut encoder = match geometry {
        Geometry::Point(_) | Geometry::MultiPoint(_) => GeomEncoder::<f64>::new(GeomType::Point),
        Geometry::LineString(_) | Geometry::MultiLineString(_) => {
            GeomEncoder::<f64>::new(GeomType::Linestring)
        }
        Geometry::Polygon(_) | Geometry::MultiPolygon(_) => {
            GeomEncoder::<f64>::new(GeomType::Polygon)
        }
        Geometry::GeometryCollection(collection) if collection.0.len() == 1 => {
            return mvt_geom_data(&collection.0[0]);
        }
        _ => return Err(mvt::Error::InvalidGeometry()),
    };

    match geometry {
        Geometry::Point(point) => add_mvt_point(&mut encoder, point.0)?,
        Geometry::MultiPoint(points) => {
            for point in &points.0 {
                add_mvt_point(&mut encoder, point.0)?;
            }
        }
        Geometry::LineString(line) => add_mvt_line(&mut encoder, line)?,
        Geometry::MultiLineString(lines) => {
            for line in &lines.0 {
                add_mvt_line(&mut encoder, line)?;
                encoder.complete_geom()?;
            }
        }
        Geometry::Polygon(polygon) => add_mvt_polygon(&mut encoder, polygon)?,
        Geometry::MultiPolygon(polygons) => {
            for polygon in &polygons.0 {
                add_mvt_polygon(&mut encoder, polygon)?;
                encoder.complete_geom()?;
            }
        }
        Geometry::GeometryCollection(_)
        | Geometry::Line(_)
        | Geometry::Rect(_)
        | Geometry::Triangle(_) => unreachable!("validated above"),
    }
    encoder.encode()
}

fn add_mvt_point(encoder: &mut GeomEncoder<f64>, coord: MvtCoord) -> Result<(), mvt::Error> {
    encoder.add_point(coord.x.into(), coord.y.into())
}

fn add_mvt_line(encoder: &mut GeomEncoder<f64>, line: &MvtLineString) -> Result<(), mvt::Error> {
    for &coord in without_trailing_duplicate(&line.0) {
        add_mvt_point(encoder, coord)?;
    }
    Ok(())
}

fn add_mvt_polygon(encoder: &mut GeomEncoder<f64>, polygon: &MvtPolygon) -> Result<(), mvt::Error> {
    add_mvt_line(encoder, polygon.exterior())?;
    encoder.complete_geom()?;
    for ring in polygon.interiors() {
        add_mvt_line(encoder, ring)?;
        encoder.complete_geom()?;
    }
    Ok(())
}

fn without_trailing_duplicate(coords: &[MvtCoord]) -> &[MvtCoord] {
    if coords.len() >= 2 && coords.first() == coords.last() {
        &coords[..coords.len() - 1]
    } else {
        coords
    }
}
