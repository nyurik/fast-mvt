use geo_types::{LineString, MultiLineString, MultiPoint, MultiPolygon, Point, Polygon};

use crate::generated::vector_tile::tile::GeomType;
use crate::geom::{Command, signed_area};
use crate::{MvtCoord, MvtError, MvtGeometry, MvtLineString, MvtResult};

/// Decodes a geometry from its command stream, read in one pass as it is decoded.
pub(crate) fn decode_geometry(
    geom_type: Option<GeomType>,
    commands: impl ExactSizeIterator<Item = u32> + Clone,
) -> MvtResult<MvtGeometry> {
    let mut commands = Commands::new(commands);
    if commands.is_empty() {
        return match geom_type {
            Some(GeomType::Point) => Ok(MultiPoint(Vec::new()).into()),
            Some(GeomType::Linestring) => Ok(MultiLineString(Vec::new()).into()),
            Some(GeomType::Polygon) => Ok(MultiPolygon(Vec::new()).into()),
            _ => Err(MvtError::InvalidGeometry),
        };
    }
    match geom_type {
        Some(GeomType::Point) => parse_points(&mut commands),
        Some(GeomType::Linestring) => parse_linestrings(&mut commands),
        Some(GeomType::Polygon) => parse_polygons(&mut commands),
        _ => Err(MvtError::InvalidGeometry),
    }
}

pub(crate) fn saturating_add_delta(value: i32, delta: u32) -> i32 {
    let v = (delta >> 1).cast_signed() ^ -(delta & 1).cast_signed();
    value.saturating_add(v)
}

/// A command stream being read, with the cursor its coordinates are relative to.
struct Commands<I> {
    /// Cheap to clone, which is how the next command is looked at without taking it.
    values: I,
    cursor: MvtCoord,
}

impl<I: ExactSizeIterator<Item = u32> + Clone> Commands<I> {
    fn new(values: I) -> Self {
        Self {
            values,
            cursor: MvtCoord::zero(),
        }
    }

    fn is_empty(&self) -> bool {
        self.values.len() == 0
    }

    /// The count of the next command, which must be `expected`.
    fn command(&mut self, expected: Command) -> MvtResult<usize> {
        let Some(value) = self.values.next() else {
            return Err(MvtError::InvalidGeometry);
        };
        match Command::decode(value)? {
            (command, count) if command == expected => Ok(count),
            _ => Err(MvtError::InvalidGeometry),
        }
    }

    /// The count of a `LineTo` command if one is next, checked to be there, or else zero.
    fn line_to(&mut self) -> MvtResult<usize> {
        let mut ahead = self.values.clone();
        let Some(Ok((Command::LineTo, count))) = ahead.next().map(Command::decode) else {
            return Ok(0);
        };
        self.values = ahead;
        self.check_coords(count)?;
        Ok(count)
    }

    /// Fails unless `count` coordinates follow, before anything is allocated for them.
    fn check_coords(&self, count: usize) -> MvtResult<()> {
        if count.saturating_mul(2) > self.values.len() {
            Err(MvtError::InvalidGeometry)
        } else {
            Ok(())
        }
    }

    fn coord(&mut self) -> MvtResult<MvtCoord> {
        // Not `ok_or`, which makes and drops an error for every coordinate.
        let (Some(dx), Some(dy)) = (self.values.next(), self.values.next()) else {
            return Err(MvtError::InvalidGeometry);
        };
        self.cursor.x = saturating_add_delta(self.cursor.x, dx);
        self.cursor.y = saturating_add_delta(self.cursor.y, dy);
        Ok(self.cursor)
    }

    /// A `MoveTo` and an optional `LineTo`, with room for `extra` more coordinates.
    fn line(&mut self, extra: usize) -> MvtResult<MvtLineString> {
        let move_count = self.command(Command::MoveTo)?;
        if move_count == 0 {
            return Err(MvtError::InvalidGeometry);
        }
        self.check_coords(move_count)?;
        let (mut coords, line_count) = if move_count == 1 {
            // The usual single `MoveTo` coordinate is held until the `LineTo` count sizes the line.
            let first = self.coord()?;
            let line_count = self.line_to()?;
            let mut coords = Vec::with_capacity(1 + line_count + extra);
            coords.push(first);
            (coords, line_count)
        } else {
            let mut coords = Vec::with_capacity(move_count + extra);
            for _ in 0..move_count {
                coords.push(self.coord()?);
            }
            let line_count = self.line_to()?;
            coords.reserve_exact(line_count + extra);
            (coords, line_count)
        };
        for _ in 0..line_count {
            coords.push(self.coord()?);
        }
        Ok(LineString(coords))
    }

    /// A closed ring, with its first coordinate repeated at the end, and its signed area.
    fn ring(&mut self) -> MvtResult<(MvtLineString, i64)> {
        // One spare slot, for the closing coordinate pushed below.
        let mut ring = self.line(1)?;
        if self.command(Command::ClosePath)? != 1 {
            return Err(MvtError::InvalidGeometry);
        }
        let area = signed_area(&ring.0);
        let first = *ring.0.first().ok_or(MvtError::InvalidGeometry)?;
        ring.0.push(first);
        Ok((ring, area))
    }
}

fn parse_points<I: ExactSizeIterator<Item = u32> + Clone>(
    commands: &mut Commands<I>,
) -> MvtResult<MvtGeometry> {
    let mut points = Vec::new();
    while !commands.is_empty() {
        let count = commands.command(Command::MoveTo)?;
        commands.check_coords(count)?;
        // A lone point, the most common case, needs no vector.
        if count == 1 && points.is_empty() && commands.values.len() == 2 {
            return Ok(Point(commands.coord()?).into());
        }
        points.reserve(count);
        for _ in 0..count {
            points.push(Point(commands.coord()?));
        }
    }
    match points.len() {
        0 => Err(MvtError::InvalidGeometry),
        1 => Ok(points.pop().ok_or(MvtError::InvalidGeometry)?.into()),
        _ => Ok(MultiPoint(points).into()),
    }
}

fn parse_linestrings<I: ExactSizeIterator<Item = u32> + Clone>(
    commands: &mut Commands<I>,
) -> MvtResult<MvtGeometry> {
    let first = commands.line(0)?;
    // A single line, the most common case, is returned without collecting into a vector.
    if commands.is_empty() {
        return Ok(first.into());
    }
    // The capacity a vector grows to on its first push anyway.
    let mut lines = Vec::with_capacity(4);
    lines.push(first);
    while !commands.is_empty() {
        lines.push(commands.line(0)?);
    }
    Ok(MultiLineString(lines).into())
}

fn parse_polygons<I: ExactSizeIterator<Item = u32> + Clone>(
    commands: &mut Commands<I>,
) -> MvtResult<MvtGeometry> {
    // The first ring is always an exterior, whatever its winding.
    let (mut exterior, _) = commands.ring()?;
    let mut interiors = Vec::new();
    // Only allocated once a second polygon starts, so a single polygon needs no vector.
    let mut polygons = Vec::new();
    while !commands.is_empty() {
        let (ring, area) = commands.ring()?;
        if area > 0 {
            let previous = std::mem::replace(&mut exterior, ring);
            polygons.push(Polygon::new(previous, std::mem::take(&mut interiors)));
        } else {
            interiors.push(ring);
        }
    }
    let last = Polygon::new(exterior, interiors);
    if polygons.is_empty() {
        Ok(last.into())
    } else {
        polygons.push(last);
        Ok(MultiPolygon(polygons).into())
    }
}

#[cfg(all(test, feature = "writer", feature = "reader"))]
mod tests {
    use std::iter::Copied;
    use std::slice::Iter;

    use geo_types::{Geometry, coord};

    use super::*;

    fn commands(data: &[u32]) -> Commands<Copied<Iter<'_, u32>>> {
        Commands::new(data.iter().copied())
    }

    fn points(data: &[u32]) -> MvtResult<MvtGeometry> {
        parse_points(&mut commands(data))
    }

    fn linestrings(data: &[u32]) -> MvtResult<MvtGeometry> {
        parse_linestrings(&mut commands(data))
    }

    fn polygons(data: &[u32]) -> MvtResult<MvtGeometry> {
        parse_polygons(&mut commands(data))
    }

    #[test]
    fn parses_move_only_lines_and_polygons() {
        let Geometry::LineString(line) = linestrings(&[9, 4, 6]).unwrap() else {
            panic!("expected line");
        };
        assert_eq!(line.0, vec![coord! { x: 2, y: 3 }]);

        let Geometry::MultiLineString(lines) = linestrings(&[9, 4, 6, 9, 2, 2]).unwrap() else {
            panic!("expected multiline");
        };
        assert_eq!(lines.0.len(), 2);

        let Geometry::Polygon(poly) = polygons(&[9, 0, 0, 15]).unwrap() else {
            panic!("expected polygon");
        };
        assert_eq!(
            poly.exterior().0,
            vec![coord! { x: 0, y: 0 }, coord! { x: 0, y: 0 }]
        );
    }

    #[test]
    fn line_and_ring_edge_paths() {
        // A line is not closed.
        assert!(matches!(
            linestrings(&[9, 0, 0, 15]),
            Err(MvtError::InvalidGeometry)
        ));
        // A line starts with a `MoveTo` that has all its coordinates.
        for data in [[10, 0, 0], [17, 0, 0], [1, 0, 0]] {
            assert!(matches!(
                commands(&data).line(0),
                Err(MvtError::InvalidGeometry)
            ));
        }
        // A ring ends with a single `ClosePath`.
        for data in [&[9, 0, 0][..], &[9, 0, 0, 8], &[9, 0, 0, 23]] {
            assert!(matches!(
                commands(data).ring(),
                Err(MvtError::InvalidGeometry)
            ));
        }

        let Geometry::Polygon(poly) = polygons(&[17, 0, 0, 10, 0, 15]).unwrap() else {
            panic!("expected polygon");
        };
        assert_eq!(poly.exterior().0.len(), 3);
    }

    #[test]
    fn invalid_geometry_streams_are_rejected() {
        assert!(matches!(
            commands(&[1]).coord(),
            Err(MvtError::InvalidGeometry)
        ));
        assert!(matches!(
            points(&[Command::MoveTo.encode(0x1fff_ffff).unwrap(), 0, 0]),
            Err(MvtError::InvalidGeometry)
        ));
        assert!(matches!(points(&[]), Err(MvtError::InvalidGeometry)));
        assert!(matches!(
            points(&[17, 1, 2]),
            Err(MvtError::InvalidGeometry)
        ));
        assert!(matches!(
            points(&[10, 0, 0]),
            Err(MvtError::InvalidGeometry)
        ));
        assert!(matches!(points(&[1]), Err(MvtError::InvalidGeometry)));
        assert!(matches!(linestrings(&[]), Err(MvtError::InvalidGeometry)));
        assert!(matches!(
            linestrings(&[9, 0]),
            Err(MvtError::InvalidGeometry)
        ));
        assert!(matches!(
            linestrings(&[9, 0, 0, 18]),
            Err(MvtError::InvalidGeometry)
        ));
        assert!(matches!(polygons(&[]), Err(MvtError::InvalidGeometry)));
        assert!(matches!(
            polygons(&[9, 0, 0, 8]),
            Err(MvtError::InvalidGeometry)
        ));
    }

    #[test]
    fn single_and_multi_geometries() {
        assert!(matches!(points(&[9, 2, 4]), Ok(Geometry::Point(_))));
        // Several `MoveTo` commands of one point each still make one point if there is only one.
        assert!(matches!(points(&[1, 9, 2, 4]), Ok(Geometry::Point(_))));
        assert!(matches!(points(&[9, 2, 4, 1]), Ok(Geometry::Point(_))));
        assert!(matches!(
            points(&[9, 2, 4, 9, 2, 4]),
            Ok(Geometry::MultiPoint(_))
        ));
        assert!(matches!(
            polygons(&[9, 0, 0, 26, 20, 0, 0, 20, 19, 0, 15, 9, 0, 0, 15]),
            Ok(Geometry::Polygon(_))
        ));
        // A ring turning the other way starts a new polygon.
        let square = [9, 0, 0, 26, 20, 0, 0, 20, 19, 0, 15];
        let reversed = [9, 0, 0, 26, 0, 20, 20, 0, 0, 19, 15];
        assert!(matches!(
            polygons(&[square, reversed, square].concat()),
            Ok(Geometry::MultiPolygon(multi)) if multi.0.len() == 2 && multi.0[0].interiors().len() == 1
        ));
    }

    #[test]
    fn ring_capacity_includes_closure_without_spare_coordinates() {
        for data in [
            &[9, 0, 0, 26, 20, 0, 0, 20, 19, 0, 15][..],
            &[9, 4, 6, 15],
            &[9, 0, 0, 34, 20, 0, 0, 20, 19, 0, 0, 19, 15],
            &[17, 0, 0, 2, 2, 10, 4, 4, 15],
        ] {
            let mut commands = commands(data);
            let (ring, _) = commands.ring().unwrap();
            assert!(commands.is_empty());
            assert_eq!(ring.0.capacity(), ring.0.len());
            assert_eq!(ring.0.first(), ring.0.last());
        }
    }

    #[test]
    fn line_capacity_does_not_include_a_closure_slot() {
        let line = commands(&[9, 0, 0, 18, 20, 0, 0, 20]).line(0).unwrap();
        assert_eq!(line.0.len(), 3);
        assert_eq!(line.0.capacity(), 3);
    }
}
