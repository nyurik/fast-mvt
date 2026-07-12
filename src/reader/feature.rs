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
    pub(crate) fn new(
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
    pub fn has_properties(self) -> bool {
        !self.feature.tags.is_empty()
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
        match self.geometry() {
            Ok(geometry) => fmt_geometry(f, &geometry)?,
            Err(error) => writeln!(f, "    geometry: <invalid geometry: {error}>")?,
        }
        write!(f, "    properties:")?;
        if self.has_properties() {
            writeln!(f)?;
            for property in self.properties() {
                match property {
                    Ok((key, value)) => writeln!(f, "      {key} = {value:?}")?,
                    Err(error) => writeln!(f, "      <invalid property: {error}>")?,
                }
            }
        } else {
            writeln!(f, " (none)")?;
        }
        Ok(())
    }
}

/// Renders the geometry after the `geometry:` label. A geometry that is a
/// single element (one point, linestring, or polygon ring) is printed inline;
/// anything with multiple elements is broken onto indented lines.
fn fmt_geometry(f: &mut fmt::Formatter<'_>, geometry: &MvtGeometry) -> fmt::Result {
    let mut lines = Vec::new();
    match geometry {
        Geometry::Point(point) => lines.push(point_line(*point)),
        Geometry::MultiPoint(points) => lines.extend(points.iter().map(|p| point_line(*p))),
        Geometry::LineString(line) => lines.push(line_line(line)),
        Geometry::MultiLineString(strings) => lines.extend(strings.iter().map(line_line)),
        Geometry::Polygon(polygon) => push_polygon(&mut lines, polygon),
        Geometry::MultiPolygon(p) => p.iter().for_each(|v| push_polygon(&mut lines, v)),
        other => lines.push(format!("{other:?}")),
    }

    if let [single] = lines.as_slice() {
        writeln!(f, "    geometry: {single}")
    } else {
        writeln!(f, "    geometry:")?;
        lines.iter().try_for_each(|v| writeln!(f, "      {v}"))
    }
}

fn point_line(point: Point<i32>) -> String {
    format!("POINT({},{})", point.x(), point.y())
}

fn line_line(line: &LineString<i32>) -> String {
    format!("LINESTRING[count={}]({})", line.0.len(), Coords(&line.0))
}

fn push_polygon(lines: &mut Vec<String>, polygon: &Polygon<i32>) {
    lines.push(ring_line(polygon.exterior(), "OUTER"));
    lines.extend(polygon.interiors().iter().map(|r| ring_line(r, "INNER")));
}

fn ring_line(ring: &LineString<i32>, winding: &str) -> String {
    format!(
        "RING[count={}]({})[{winding}]",
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
        assert!(!feature.has_properties());
        assert!(matches!(feature.geometry(), Err(MvtError::InvalidGeometry)));
    }
}
