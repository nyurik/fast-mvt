use crate::generated::vector_tile::tile::GeomType;
use crate::geom::{Command, signed_area};
use crate::{MvtCoord, MvtError, MvtGeometry, MvtPolygon, MvtResult};

pub(crate) fn encode_parameter(value: i32) -> u32 {
    ((value << 1) ^ (value >> 31)).cast_unsigned()
}

/// The geometry commands of one feature. Kept from feature to feature so its buffers are reused.
#[derive(Debug, Default)]
pub(crate) struct GeometryBuf {
    pub(crate) data: Vec<u32>,
    cursor: MvtCoord,
    /// A ring given as an iterator, gathered to check its winding.
    ring: Vec<MvtCoord>,
}

impl GeometryBuf {
    pub(crate) fn clear(&mut self) {
        self.data.clear();
        self.cursor = MvtCoord { x: 0, y: 0 };
    }

    /// Appends `geometry`, returning its MVT type.
    pub(crate) fn push_geometry(&mut self, geometry: &MvtGeometry) -> MvtResult<GeomType> {
        Ok(match geometry {
            MvtGeometry::Point(point) => {
                self.points([point.0])?;
                GeomType::Point
            }
            MvtGeometry::MultiPoint(points) => {
                self.points(points.0.iter().map(|point| point.0))?;
                GeomType::Point
            }
            MvtGeometry::LineString(line) => {
                self.line(line.0.iter().copied())?;
                GeomType::Linestring
            }
            MvtGeometry::MultiLineString(lines) => {
                for line in &lines.0 {
                    self.line(line.0.iter().copied())?;
                }
                GeomType::Linestring
            }
            MvtGeometry::Polygon(polygon) => {
                self.polygon(polygon)?;
                GeomType::Polygon
            }
            MvtGeometry::MultiPolygon(polygons) => {
                for polygon in &polygons.0 {
                    self.polygon(polygon)?;
                }
                GeomType::Polygon
            }
            MvtGeometry::GeometryCollection(collection) if collection.0.len() == 1 => {
                self.push_geometry(&collection.0[0])?
            }
            MvtGeometry::GeometryCollection(_) => Err(MvtError::UnsupportedGeometry(
                "GeometryCollection with multiple items",
            ))?,
            MvtGeometry::Line(_) => Err(MvtError::UnsupportedGeometry("Line"))?,
            MvtGeometry::Rect(_) => Err(MvtError::UnsupportedGeometry("Rect"))?,
            MvtGeometry::Triangle(_) => Err(MvtError::UnsupportedGeometry("Triangle"))?,
        })
    }

    /// One `MoveTo` holding every point, or nothing for no points.
    pub(crate) fn points(&mut self, coords: impl IntoIterator<Item = MvtCoord>) -> MvtResult<()> {
        self.command(Command::MoveTo, coords)
    }

    pub(crate) fn line(&mut self, coords: impl IntoIterator<Item = MvtCoord>) -> MvtResult<()> {
        let mut coords = coords.into_iter();
        let first = coords.next().ok_or(MvtError::InvalidGeometry)?;
        self.command(Command::MoveTo, [first])?;
        self.command(Command::LineTo, coords)?;
        Ok(())
    }

    /// Appends `command` with the deltas of `coords`. Their count is only known at the end, so the
    /// command word is filled in last; with no coordinates, nothing is written.
    fn command(
        &mut self,
        command: Command,
        coords: impl IntoIterator<Item = MvtCoord>,
    ) -> MvtResult<()> {
        let start = self.data.len();
        self.data.push(0);
        let mut count = 0_usize;
        for coord in coords {
            self.push_delta(coord);
            count += 1;
        }
        if count == 0 {
            self.data.truncate(start);
        } else {
            self.data[start] = command.encode(u32_index(count)?)?;
        }
        Ok(())
    }

    /// A polygon ring, rewound if needed so that exterior and interior rings turn opposite ways.
    pub(crate) fn ring(
        &mut self,
        coords: impl IntoIterator<Item = MvtCoord>,
        exterior: bool,
    ) -> MvtResult<()> {
        let mut ring = std::mem::take(&mut self.ring);
        ring.clear();
        ring.extend(coords);
        let result = self.ring_slice(&ring, exterior);
        self.ring = ring;
        result
    }

    fn polygon(&mut self, polygon: &MvtPolygon) -> MvtResult<()> {
        self.ring_slice(&polygon.exterior().0, true)?;
        for ring in polygon.interiors() {
            self.ring_slice(&ring.0, false)?;
        }
        Ok(())
    }

    fn ring_slice(&mut self, coords: &[MvtCoord], exterior: bool) -> MvtResult<()> {
        let coords = without_trailing_duplicate(coords);
        if coords.is_empty() {
            return Err(MvtError::InvalidGeometry);
        }
        let area = signed_area(coords);
        let reverse = area != 0 && (area > 0) != exterior;
        self.data.push(Command::MoveTo.encode(1)?);
        self.push_delta(ring_coord(coords, 0, reverse));
        if coords.len() > 1 {
            self.data
                .push(Command::LineTo.encode(u32_index(coords.len() - 1)?)?);
            for idx in 1..coords.len() {
                self.push_delta(ring_coord(coords, idx, reverse));
            }
        }
        self.data.push(Command::ClosePath.encode(1)?);
        Ok(())
    }

    fn push_delta(&mut self, coord: MvtCoord) {
        self.data
            .push(encode_parameter(coord.x.saturating_sub(self.cursor.x)));
        self.data
            .push(encode_parameter(coord.y.saturating_sub(self.cursor.y)));
        self.cursor = coord;
    }
}

fn without_trailing_duplicate(coords: &[MvtCoord]) -> &[MvtCoord] {
    if coords.len() >= 2 && coords.first() == coords.last() {
        &coords[..coords.len() - 1]
    } else {
        coords
    }
}

fn ring_coord(coords: &[MvtCoord], idx: usize, reverse: bool) -> MvtCoord {
    if reverse {
        coords[coords.len() - 1 - idx]
    } else {
        coords[idx]
    }
}

pub(crate) fn u32_index(value: usize) -> MvtResult<u32> {
    u32::try_from(value).map_err(|_| MvtError::IndexOverflow(value))
}

#[cfg(test)]
mod tests {
    use geo_types::{
        GeometryCollection, Line, MultiLineString, MultiPoint, MultiPolygon, Rect, Triangle, coord,
        line_string, point, polygon,
    };

    use super::*;

    fn encode_geometry(geometry: &MvtGeometry) -> MvtResult<(GeomType, Vec<u32>)> {
        let mut buf = GeometryBuf::default();
        let geom_type = buf.push_geometry(geometry)?;
        Ok((geom_type, buf.data))
    }

    #[test]
    fn encodes_spec_point() {
        let geometry = MvtGeometry::Point(point! { x: 25, y: 17 });
        let (_, data) = encode_geometry(&geometry).unwrap();
        assert_eq!(data, vec![9, 50, 34]);
    }

    #[test]
    fn encodes_spec_linestring() {
        let geometry =
            MvtGeometry::LineString(line_string![(x: 2, y: 2), (x: 2, y: 10), (x: 10, y: 10)]);
        let (_, data) = encode_geometry(&geometry).unwrap();
        assert_eq!(data, vec![9, 4, 4, 18, 0, 16, 16, 0]);
    }

    #[test]
    fn encodes_spec_polygon() {
        let geometry = MvtGeometry::Polygon(polygon![(x: 3, y: 6), (x: 8, y: 12), (x: 20, y: 34)]);
        let (_, data) = encode_geometry(&geometry).unwrap();
        assert_eq!(data, vec![9, 6, 12, 18, 10, 12, 24, 44, 15]);
    }

    #[test]
    fn encodes_empty_collections_and_geometry_collection_delegate() {
        assert_eq!(
            encode_geometry(&MvtGeometry::MultiPoint(MultiPoint(vec![]))).unwrap(),
            (GeomType::Point, Vec::new())
        );
        assert_eq!(
            encode_geometry(&MvtGeometry::MultiLineString(MultiLineString(vec![]))).unwrap(),
            (GeomType::Linestring, Vec::new())
        );
        assert_eq!(
            encode_geometry(&MvtGeometry::MultiPolygon(MultiPolygon(vec![]))).unwrap(),
            (GeomType::Polygon, Vec::new())
        );
        let collection =
            MvtGeometry::GeometryCollection(GeometryCollection(vec![MvtGeometry::Point(
                point! { x: 1, y: 2 },
            )]));
        let direct = encode_geometry(&MvtGeometry::Point(point! { x: 1, y: 2 })).unwrap();
        assert_eq!(encode_geometry(&collection).unwrap(), direct);
    }

    #[test]
    fn unsupported_and_invalid_geometries_are_errors() {
        let collection = MvtGeometry::GeometryCollection(GeometryCollection(vec![
            MvtGeometry::Point(point! { x: 0, y: 0 }),
            MvtGeometry::Point(point! { x: 1, y: 1 }),
        ]));
        assert!(matches!(
            encode_geometry(&collection),
            Err(MvtError::UnsupportedGeometry(_))
        ));
        assert!(matches!(
            encode_geometry(&MvtGeometry::Line(Line::new((0, 0), (1, 1)))),
            Err(MvtError::UnsupportedGeometry("Line"))
        ));
        assert!(matches!(
            encode_geometry(&MvtGeometry::Rect(Rect::new((0, 0), (1, 1)))),
            Err(MvtError::UnsupportedGeometry("Rect"))
        ));
        assert!(matches!(
            encode_geometry(&MvtGeometry::Triangle(Triangle(
                coord! { x: 0, y: 0 },
                coord! { x: 1, y: 0 },
                coord! { x: 0, y: 1 },
            ))),
            Err(MvtError::UnsupportedGeometry("Triangle"))
        ));
        assert!(matches!(
            encode_geometry(&MvtGeometry::MultiPoint(MultiPoint(vec![
                point! { x: 1, y: 1 }
            ]))),
            Ok((GeomType::Point, _))
        ));
        assert!(matches!(
            encode_geometry(&MvtGeometry::LineString(line_string![])),
            Err(MvtError::InvalidGeometry)
        ));
        assert!(matches!(
            encode_geometry(&MvtGeometry::Polygon(polygon![])),
            Err(MvtError::InvalidGeometry)
        ));
        assert_eq!(signed_area(&[]), 0);
    }

    #[test]
    fn encoder_edge_cases() {
        // No points is no geometry.
        let mut geometry = GeometryBuf::default();
        geometry.points(std::iter::empty::<MvtCoord>()).unwrap();
        assert_eq!(geometry.data, Vec::<u32>::new());
        // Single-vertex line and ring skip the `LineTo` branch.
        encode_geometry(&MvtGeometry::LineString(line_string![(x: 1, y: 2)])).unwrap();
        encode_geometry(&MvtGeometry::Polygon(polygon![(x: 1, y: 2)])).unwrap();
        // A reversed exterior ring is rewound (exercises the `reverse` path).
        encode_geometry(&MvtGeometry::Polygon(
            polygon![(x: 20, y: 34), (x: 8, y: 12), (x: 3, y: 6)],
        ))
        .unwrap();
    }
}
