//! Builds a tile that exercises every property value type and stores the
//! encoded bytes as an insta binary snapshot. The snapshot doubles as a
//! fixture for the `mvt dump` CLI (see the `dump-fixture` justfile recipe).
#![cfg(all(feature = "reader", feature = "writer"))]

use fast_mvt::{MvtGeometry, MvtTileBuilder};

/// Encode a single feature carrying one property of every writable MVT value
/// type. Note that `MvtValueRef::Null` has no writable counterpart: the builder
/// intentionally drops null tags, so it cannot appear in a fixture.
#[test]
fn tile_with_every_property_type() {
    let mut feature = MvtTileBuilder::new()
        .layer("all_types")
        .unwrap()
        .feature(&MvtGeometry::Point((10, 20).into()))
        .unwrap();
    feature.id(Some(42));
    feature.tag_string("string", "hello").unwrap();
    feature.tag_float("float", 1.25).unwrap();
    feature.tag_double("double", 2.5).unwrap();
    feature.tag_int("int", -3).unwrap();
    feature.tag_uint("uint", 4).unwrap();
    feature.tag_sint("sint", -5).unwrap();
    feature.tag_bool("bool", true).unwrap();

    let bytes = feature.finish().finish().finish();

    insta::assert_binary_snapshot!("all_property_types.mvt", bytes);
}
