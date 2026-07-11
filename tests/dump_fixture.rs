//! Builds a tile exercising every property value type and every geometry type,
//! then locks it down two ways: the encoded bytes as an insta binary snapshot,
//! and the human-readable `MvtReaderRef` `Debug` dump as an inline snapshot.
#![cfg(all(feature = "reader", feature = "writer"))]

use fast_mvt::{MvtGeometry, MvtReaderRef, MvtResult, MvtTileBuilder, MvtValue};
use geo_types::{MultiLineString, MultiPoint, MultiPolygon, line_string, point, polygon};

/// Cover every writable property type and every geometry type, and probe the
/// two edge cases the reader exposes:
///
/// * **Null property** — there is no way to *store* one. `tag()` drops
///   `MvtValue::Null` (see the writer), so the `null` attempt below never
///   reaches the encoded tile and is absent from the dump.
/// * **Feature id** — round-trips as-is; the `LineString` feature omits it to
///   show the `(none)` rendering.
#[test]
#[expect(clippy::panic_in_result_fn)]
fn tile_with_every_property_and_geometry_type() -> MvtResult<()> {
    // A point feature carrying one property of every writable value type, plus
    // an id and a null tag that the builder silently drops.
    let mut feature = MvtTileBuilder::new()
        .layer("everything")?
        .feature(&MvtGeometry::Point(point! { x: 10, y: 20 }))?;
    feature
        .id(Some(1))
        .tag_string("string", "hello")?
        .tag_float("float", 1.25)?
        .tag_double("double", 2.5)?
        .tag_int("int", -3)?
        .tag_uint("uint", 4)?
        .tag_sint("sint", -5)?
        .tag_bool("bool", true)?
        .tag("null", MvtValue::Null)?;
    assert_eq!(
        feature.num_tags(),
        7,
        "null tag must be dropped, not stored"
    );
    let layer = feature.finish();

    // One feature per remaining geometry type; the linestring omits its id.
    let mut feature = layer.feature(&MvtGeometry::MultiPoint(MultiPoint(vec![
        point! { x: 1, y: 2 },
        point! { x: 3, y: 4 },
    ])))?;
    feature.id(Some(2));
    let layer = feature.finish();

    let layer = layer
        .feature(&MvtGeometry::LineString(
            line_string![(x: 0, y: 0), (x: 5, y: 5), (x: 10, y: 0)],
        ))?
        .finish();

    let mut feature = layer.feature(&MvtGeometry::MultiLineString(MultiLineString(vec![
        line_string![(x: 0, y: 0), (x: 1, y: 1)],
        line_string![(x: 2, y: 2), (x: 3, y: 3)],
    ])))?;
    feature.id(Some(4));
    let layer = feature.finish();

    let mut feature = layer.feature(&MvtGeometry::Polygon(polygon!(
        exterior: [(x: 0, y: 0), (x: 10, y: 0), (x: 10, y: 10), (x: 0, y: 10), (x: 0, y: 0)],
        interiors: [[(x: 3, y: 3), (x: 3, y: 6), (x: 6, y: 6), (x: 6, y: 3), (x: 3, y: 3)]],
    )))?;
    feature.id(Some(5));
    let layer = feature.finish();

    let mut feature = layer.feature(&MvtGeometry::MultiPolygon(MultiPolygon(vec![
        polygon![(x: 0, y: 0), (x: 4, y: 0), (x: 4, y: 4), (x: 0, y: 4), (x: 0, y: 0)],
        polygon![(x: 6, y: 6), (x: 9, y: 6), (x: 9, y: 9), (x: 6, y: 9), (x: 6, y: 6)],
    ])))?;
    feature.id(Some(6));
    let layer = feature.finish();

    let bytes = layer.finish().finish();

    insta::assert_binary_snapshot!("tile.mvt", bytes.clone());

    let reader = MvtReaderRef::new(&bytes)?;
    insta::assert_debug_snapshot!(reader, @r#"
    layer: 0
      name: everything
      version: 2
      extent: 4096
      feature: 0
        id: 1
        geometry: POINT(10,20)
        properties:
          string = "hello"
          float = 1.25 (float)
          double = 2.5 (double)
          int = -3 (int)
          uint = 4 (uint)
          sint = -5 (sint)
          bool = true (bool)
      feature: 1
        id: 2
        geometry:
          POINT(1,2)
          POINT(3,4)
        properties: (none)
      feature: 2
        id: (none)
        geometry: LINESTRING[count=3](0 0,5 5,10 0)
        properties: (none)
      feature: 3
        id: 4
        geometry:
          LINESTRING[count=2](0 0,1 1)
          LINESTRING[count=2](2 2,3 3)
        properties: (none)
      feature: 4
        id: 5
        geometry:
          RING[count=5](0 0,10 0,10 10,0 10,0 0)[OUTER]
          RING[count=5](3 3,3 6,6 6,6 3,3 3)[INNER]
        properties: (none)
      feature: 5
        id: 6
        geometry:
          RING[count=5](0 0,4 0,4 4,0 4,0 0)[OUTER]
          RING[count=5](6 6,9 6,9 9,6 9,6 6)[OUTER]
        properties: (none)
    "#);
    Ok(())
}
