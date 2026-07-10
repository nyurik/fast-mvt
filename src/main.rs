use std::io;
use std::path::PathBuf;

use clap::{Parser, Subcommand};
use fast_mvt::proto::GeomType;
use fast_mvt::{MvtGeometry, MvtReaderRef, MvtResult, MvtValueRef};
use geo_types::{Coord, LineString, MultiLineString, MultiPoint, MultiPolygon, Point, Polygon};

#[derive(Debug, Parser)]
#[command(author, version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Dump a vector tile in a human-readable form.
    Dump {
        /// MVT file to dump.
        file: PathBuf,
    },
}

fn main() -> MvtResult<()> {
    run(Cli::parse())
}

fn run(cli: Cli) -> MvtResult<()> {
    match cli.command {
        Command::Dump { file } => dump_file(file),
    }
}

fn dump_file(file: PathBuf) -> MvtResult<()> {
    let data = std::fs::read(file)?;
    let reader = MvtReaderRef::new(&data)?;
    let stdout = io::stdout();
    let mut out = io::BufWriter::new(stdout.lock());
    dump_reader(&reader, &mut out)
}

fn dump_reader(writer: &MvtReaderRef<'_>, out: &mut impl io::Write) -> MvtResult<()> {
    for (layer_index, layer) in writer.layers().enumerate() {
        if layer_index > 0 {
            writeln!(
                out,
                "============================================================="
            )?;
        }
        writeln!(out, "layer: {layer_index}")?;
        writeln!(out, "  name: {}", layer.name())?;
        writeln!(out, "  version: {}", layer.version())?;
        writeln!(out, "  extent: {}", layer.extent())?;

        for (feature_index, feature) in layer.features().enumerate() {
            writeln!(out, "  feature: {feature_index}")?;
            write!(out, "    id: ")?;
            match feature.id() {
                Some(id) => writeln!(out, "{id}")?,
                None => writeln!(out, "(none)")?,
            }
            let geom_type = format_geom_type(feature.geom_type());
            writeln!(out, "    geometry: {geom_type}")?;
            write_geometry(out, &feature.geometry()?)?;
            writeln!(out, "    properties:")?;
            for property in feature.properties() {
                let (key, value) = property?;
                let type_hint = match value_type(value) {
                    Some(ty) => format!(" ({ty})"),
                    None => String::new(),
                };
                writeln!(out, "      {key}{type_hint} = {}", format_value(value))?;
            }
        }
    }
    Ok(())
}

fn format_geom_type(value: Option<GeomType>) -> &'static str {
    match value {
        Some(GeomType::Point) => "point",
        Some(GeomType::Linestring) => "linestring",
        Some(GeomType::Polygon) => "polygon",
        Some(GeomType::Unknown) | None => "unknown",
    }
}

fn value_type(value: MvtValueRef<'_>) -> Option<&'static str> {
    match value {
        MvtValueRef::String(_) => None,
        MvtValueRef::Float(_) => Some("float"),
        MvtValueRef::Double(_) => Some("double"),
        MvtValueRef::Int(_) => Some("int"),
        MvtValueRef::UInt(_) => Some("uint"),
        MvtValueRef::SInt(_) => Some("sint"),
        MvtValueRef::Bool(_) => Some("bool"),
        MvtValueRef::Null => Some("null"),
    }
}

fn format_value(value: MvtValueRef<'_>) -> String {
    match value {
        MvtValueRef::String(value) => format!("{value:?}"),
        MvtValueRef::Float(value) => value.to_string(),
        MvtValueRef::Double(value) => value.to_string(),
        MvtValueRef::Int(value) | MvtValueRef::SInt(value) => value.to_string(),
        MvtValueRef::UInt(value) => value.to_string(),
        MvtValueRef::Bool(value) => value.to_string(),
        MvtValueRef::Null => "null".to_string(),
    }
}

fn write_geometry(out: &mut impl io::Write, geometry: &MvtGeometry) -> io::Result<()> {
    match geometry {
        MvtGeometry::Point(point) => write_point(out, *point),
        MvtGeometry::MultiPoint(points) => write_points(out, points),
        MvtGeometry::LineString(line) => write_line(out, line),
        MvtGeometry::MultiLineString(lines) => write_lines(out, lines),
        MvtGeometry::Polygon(polygon) => write_polygon(out, polygon),
        MvtGeometry::MultiPolygon(polygons) => write_polygons(out, polygons),
        geometry => writeln!(out, "      {geometry:?}"),
    }
}

fn write_point(out: &mut impl io::Write, point: Point<i32>) -> io::Result<()> {
    writeln!(out, "      POINT({},{})", point.x(), point.y())
}

fn write_points(out: &mut impl io::Write, points: &MultiPoint<i32>) -> io::Result<()> {
    for point in points {
        write_point(out, *point)?;
    }
    Ok(())
}

fn write_line(out: &mut impl io::Write, line: &LineString<i32>) -> io::Result<()> {
    let count = line.0.len();
    let coords = format_coords(&line.0);
    writeln!(out, "      LINESTRING[count={count}]({coords})")
}

fn write_lines(out: &mut impl io::Write, lines: &MultiLineString<i32>) -> io::Result<()> {
    for line in lines {
        write_line(out, line)?;
    }
    Ok(())
}

fn write_polygon(out: &mut impl io::Write, polygon: &Polygon<i32>) -> io::Result<()> {
    write_ring(out, polygon.exterior(), "OUTER")?;
    for ring in polygon.interiors() {
        write_ring(out, ring, "INNER")?;
    }
    Ok(())
}

fn write_polygons(out: &mut impl io::Write, polygons: &MultiPolygon<i32>) -> io::Result<()> {
    for polygon in polygons {
        write_polygon(out, polygon)?;
    }
    Ok(())
}

fn write_ring(
    out: &mut impl io::Write,
    ring: &LineString<i32>,
    winding: &'static str,
) -> io::Result<()> {
    writeln!(
        out,
        "      RING[count={}]({})[{winding}]",
        ring.0.len(),
        format_coords(&ring.0)
    )
}

fn format_coords(coords: &[Coord<i32>]) -> String {
    coords
        .iter()
        .map(|coord| format!("{} {}", coord.x, coord.y))
        .collect::<Vec<_>>()
        .join(",")
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "writer")]
    use fast_mvt::MvtTileBuilder;
    use fast_mvt::MvtValueRef;
    use geo_types::{Geometry, GeometryCollection};
    use tempfile::NamedTempFile;

    use super::*;

    #[test]
    #[cfg(feature = "writer")]
    fn dump_reader_writes_layer_feature_geometry_and_properties() {
        let layer = MvtTileBuilder::new()
            .layer("places")
            .unwrap()
            .feature(&MvtGeometry::Point((1, 2).into()))
            .unwrap();
        let mut feature = layer;
        feature.tag("name", "Example").unwrap();
        feature.tag("visible", true).unwrap();
        let bytes = feature.finish().finish().finish();
        let reader = MvtReaderRef::new(&bytes).unwrap();
        let mut out = Vec::new();

        dump_reader(&reader, &mut out).unwrap();
        let out = String::from_utf8(out).unwrap();

        assert!(out.contains("  name: places"));
        assert!(out.contains("  feature: 0"));
        assert!(out.contains("    id: (none)"));
        assert!(out.contains("    geometry: point"));
        assert!(out.contains("      POINT(1,2)"));
        assert!(out.contains("      name = \"Example\""));
        assert!(out.contains("      visible (bool) = true"));
    }

    #[test]
    fn run_dump_reads_tile_file() {
        let bytes = Vec::new();
        let mut file = NamedTempFile::new().unwrap();
        io::Write::write_all(&mut file, &bytes).unwrap();

        run(Cli {
            command: Command::Dump {
                file: file.path().to_path_buf(),
            },
        })
        .unwrap();
    }

    #[test]
    fn formatting_helpers_cover_value_and_geometry_variants() {
        assert_eq!(format_geom_type(Some(GeomType::Point)), "point");
        assert_eq!(format_geom_type(Some(GeomType::Linestring)), "linestring");
        assert_eq!(format_geom_type(Some(GeomType::Polygon)), "polygon");
        assert_eq!(format_geom_type(Some(GeomType::Unknown)), "unknown");
        assert_eq!(format_geom_type(None), "unknown");

        assert_eq!(format_value(MvtValueRef::String("x")), "\"x\"");
        assert_eq!(format_value(MvtValueRef::Float(1.25)), "1.25");
        assert_eq!(format_value(MvtValueRef::Double(2.5)), "2.5");
        assert_eq!(format_value(MvtValueRef::Int(-3)), "-3");
        assert_eq!(format_value(MvtValueRef::UInt(4)), "4");
        assert_eq!(format_value(MvtValueRef::SInt(-5)), "-5");
        assert_eq!(format_value(MvtValueRef::Bool(true)), "true");
        assert_eq!(format_value(MvtValueRef::Null), "null");

        assert_eq!(value_type(MvtValueRef::String("x")), None);
        assert_eq!(value_type(MvtValueRef::Float(1.25)), Some("float"));
        assert_eq!(value_type(MvtValueRef::Double(2.5)), Some("double"));
        assert_eq!(value_type(MvtValueRef::Int(-3)), Some("int"));
        assert_eq!(value_type(MvtValueRef::UInt(4)), Some("uint"));
        assert_eq!(value_type(MvtValueRef::SInt(-5)), Some("sint"));
        assert_eq!(value_type(MvtValueRef::Bool(true)), Some("bool"));
        assert_eq!(value_type(MvtValueRef::Null), Some("null"));

        let mut out = Vec::new();
        write_geometry(
            &mut out,
            &MvtGeometry::MultiPoint(MultiPoint(vec![Point::new(1, 2), Point::new(3, 4)])),
        )
        .unwrap();
        write_geometry(
            &mut out,
            &MvtGeometry::LineString(LineString(vec![(1, 2).into(), (3, 4).into()])),
        )
        .unwrap();
        write_geometry(
            &mut out,
            &MvtGeometry::MultiLineString(MultiLineString(vec![LineString(vec![
                (1, 2).into(),
                (3, 4).into(),
            ])])),
        )
        .unwrap();
        write_geometry(
            &mut out,
            &MvtGeometry::Polygon(Polygon::new(
                LineString(vec![(0, 0).into(), (1, 0).into(), (0, 1).into()]),
                vec![LineString(vec![(0, 0).into(), (0, 0).into()])],
            )),
        )
        .unwrap();
        write_geometry(
            &mut out,
            &MvtGeometry::MultiPolygon(
                vec![Polygon::new(
                    LineString(vec![(0, 0).into(), (1, 0).into(), (0, 1).into()]),
                    vec![],
                )]
                .into(),
            ),
        )
        .unwrap();
        write_geometry(
            &mut out,
            &Geometry::GeometryCollection(GeometryCollection(vec![])),
        )
        .unwrap();

        let out = String::from_utf8(out).unwrap();
        assert!(out.contains("POINT(1,2)"));
        assert!(out.contains("LINESTRING[count=2](1 2,3 4)"));
        assert!(out.contains("[OUTER]"));
        assert!(out.contains("[INNER]"));
    }
}
