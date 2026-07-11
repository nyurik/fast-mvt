use std::fmt::{self, Write as _};

use buffa::Enumeration as _;
use geo_types::{Coord, Geometry, LineString, Point, Polygon};

use super::property::{MvtPropertyIter, MvtValueRef};
use crate::generated::vector_tile::tile as proto_tile;
use crate::geom_reader::decode_geometry;
use crate::{MvtFeature, MvtGeometry, MvtResult};

#[derive(Copy, Clone)]
pub struct MvtFeatureRef<'a> {
    layer: &'a proto_tile::LayerView<'a>,
    feature: &'a proto_tile::FeatureView<'a>,
}

impl<'a> MvtFeatureRef<'a> {
    pub(super) fn new(
        layer: &'a proto_tile::LayerView<'a>,
        feature: &'a proto_tile::FeatureView<'a>,
    ) -> Self {
        Self { layer, feature }
    }

    #[must_use]
    pub fn id(self) -> Option<u64> {
        self.feature.id
    }

    #[must_use]
    pub fn tags(self) -> &'a [u32] {
        &self.feature.tags
    }

    #[must_use]
    pub fn geometry_commands(self) -> &'a [u32] {
        &self.feature.geometry
    }

    #[must_use]
    pub fn geom_type(self) -> Option<proto_tile::GeomType> {
        self.feature.r#type
    }

    #[must_use]
    pub fn geom_type_value(self) -> Option<i32> {
        self.feature.r#type.map(|v| v.to_i32())
    }

    #[must_use]
    pub fn properties(self) -> MvtPropertyIter<'a> {
        MvtPropertyIter::new(
            &self.layer.keys,
            &self.layer.values,
            self.feature.tags.chunks(2),
        )
    }

    pub fn properties_vec(self) -> MvtResult<Vec<(&'a str, MvtValueRef<'a>)>> {
        self.properties().collect()
    }

    pub fn geometry(self) -> MvtResult<MvtGeometry> {
        decode_geometry(self.geom_type(), &self.feature.geometry)
    }

    pub fn to_feature(self) -> MvtResult<MvtFeature> {
        let properties = self
            .properties()
            .map(|property| property.map(|(key, value)| (key.to_string(), value.into_owned())))
            .collect::<MvtResult<Vec<_>>>()?;
        Ok(MvtFeature {
            id: self.id(),
            geometry: self.geometry()?,
            properties,
        })
    }
}

impl fmt::Debug for MvtFeatureRef<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.id() {
            Some(id) => writeln!(f, "    id: {id}")?,
            None => writeln!(f, "    id: (none)")?,
        }
        writeln!(
            f,
            "    geometry: {}",
            match self.geom_type() {
                Some(proto_tile::GeomType::Point) => "point",
                Some(proto_tile::GeomType::Linestring) => "linestring",
                Some(proto_tile::GeomType::Polygon) => "polygon",
                Some(proto_tile::GeomType::Unknown) | None => "unknown",
            }
        )?;
        match self.geometry() {
            Ok(geometry) => fmt_geometry(f, &geometry)?,
            Err(error) => writeln!(f, "      <invalid geometry: {error}>")?,
        }
        writeln!(f, "    properties:")?;
        for property in self.properties() {
            match property {
                Ok((key, value)) => match value.type_name() {
                    Some(ty) => writeln!(f, "      {key} ({ty}) = {value:?}")?,
                    None => writeln!(f, "      {key} = {value:?}")?,
                },
                Err(error) => writeln!(f, "      <invalid property: {error}>")?,
            }
        }
        Ok(())
    }
}

fn fmt_geometry(f: &mut fmt::Formatter<'_>, geometry: &MvtGeometry) -> fmt::Result {
    match geometry {
        Geometry::Point(point) => fmt_point(f, *point),
        Geometry::MultiPoint(points) => points.iter().try_for_each(|point| fmt_point(f, *point)),
        Geometry::LineString(line) => fmt_line(f, line),
        Geometry::MultiLineString(lines) => lines.iter().try_for_each(|line| fmt_line(f, line)),
        Geometry::Polygon(polygon) => fmt_polygon(f, polygon),
        Geometry::MultiPolygon(polygons) => polygons
            .iter()
            .try_for_each(|polygon| fmt_polygon(f, polygon)),
        other => writeln!(f, "      {other:?}"),
    }
}

fn fmt_point(f: &mut fmt::Formatter<'_>, point: Point<i32>) -> fmt::Result {
    writeln!(f, "      POINT({},{})", point.x(), point.y())
}

fn fmt_line(f: &mut fmt::Formatter<'_>, line: &LineString<i32>) -> fmt::Result {
    writeln!(
        f,
        "      LINESTRING[count={}]({})",
        line.0.len(),
        Coords(&line.0)
    )
}

fn fmt_polygon(f: &mut fmt::Formatter<'_>, polygon: &Polygon<i32>) -> fmt::Result {
    fmt_ring(f, polygon.exterior(), "OUTER")?;
    polygon
        .interiors()
        .iter()
        .try_for_each(|ring| fmt_ring(f, ring, "INNER"))
}

fn fmt_ring(f: &mut fmt::Formatter<'_>, ring: &LineString<i32>, winding: &str) -> fmt::Result {
    writeln!(
        f,
        "      RING[count={}]({})[{winding}]",
        ring.0.len(),
        Coords(&ring.0)
    )
}

/// Renders a coordinate slice as `x y,x y,...`.
struct Coords<'a>(&'a [Coord<i32>]);

impl fmt::Display for Coords<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, coord) in self.0.iter().enumerate() {
            if index > 0 {
                f.write_char(',')?;
            }
            write!(f, "{} {}", coord.x, coord.y)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use geo_types::{Geometry, MultiLineString, MultiPoint, MultiPolygon};

    use super::*;
    use crate::MvtError;
    use crate::reader::tests::reader_from_layer;

    #[test]
    fn empty_geometries_keep_declared_type() {
        let layer = proto_tile::Layer {
            version: 2,
            name: "empty".into(),
            extent: Some(crate::DEFAULT_EXTENT.get()),
            features: vec![],
            ..Default::default()
        };
        for (geom_type, expected) in [
            (
                proto_tile::GeomType::Point,
                Geometry::MultiPoint(MultiPoint(vec![])),
            ),
            (
                proto_tile::GeomType::Linestring,
                Geometry::MultiLineString(MultiLineString(vec![])),
            ),
            (
                proto_tile::GeomType::Polygon,
                Geometry::MultiPolygon(MultiPolygon(vec![])),
            ),
        ] {
            let feature = proto_tile::Feature {
                r#type: Some(geom_type),
                geometry: vec![],
                ..Default::default()
            };
            let mut layer = layer.clone();
            layer.features = vec![feature];
            let reader = reader_from_layer(layer);
            let feature = reader.layers().next().unwrap().features().next().unwrap();
            assert_eq!(feature.geometry().unwrap(), expected);
        }

        let feature = proto_tile::Feature {
            r#type: Some(proto_tile::GeomType::Unknown),
            geometry: vec![],
            ..Default::default()
        };
        let mut layer = layer.clone();
        layer.features = vec![feature];
        let reader = reader_from_layer(layer);
        let feature = reader.layers().next().unwrap().features().next().unwrap();
        assert!(matches!(feature.geometry(), Err(MvtError::InvalidGeometry)));
    }
}

#[cfg(all(test, feature = "writer"))]
mod writer_tests {
    use crate::{MvtGeometry, MvtReaderRef, MvtTileBuilder};

    #[test]
    fn feature_ref_debug_renders_a_self_contained_block() {
        let mut feature = MvtTileBuilder::new()
            .layer("places")
            .unwrap()
            .feature(&MvtGeometry::Point((1, 2).into()))
            .unwrap();
        feature.id(Some(7));
        feature.tag_string("name", "Example").unwrap();
        let bytes = feature.finish().finish().finish();

        let reader = MvtReaderRef::new(&bytes).expect("valid MVT bytes");
        let feature = reader.layers().next().unwrap().features().next().unwrap();

        // A feature's `Debug` is its own impl, usable independently of the
        // surrounding tile. Each line is newline-terminated.
        assert_eq!(
            format!("{feature:?}"),
            "    id: 7\n    geometry: point\n      POINT(1,2)\n    properties:\n      name = \"Example\"\n"
        );
    }
}
