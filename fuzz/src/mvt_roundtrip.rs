use buffa::Message as _;
use fast_mvt::proto::Tile;
use fast_mvt::{MvtGeomType, MvtGeometry, MvtLayerBuilder, MvtReaderRef, MvtResult, MvtTile};

/// Fuzz input exercising `Tile protobuf -> MvtTile -> bytes -> MvtTile`.
///
/// The first public round trip is normalizing: unsupported geometry streams,
/// invalid layer metadata, and null-valued tags may be rejected or canonicalized.
/// Once normalized, subsequent round trips must be fixpoints.
///
/// The written bytes are also checked against the generated protobuf encoder, and against the same
/// tile written piece by piece with the streaming builder API.
pub struct MvtRoundtripInput {
    pub tile: Tile,
}

impl arbitrary::Arbitrary<'_> for MvtRoundtripInput {
    fn arbitrary(u: &mut arbitrary::Unstructured<'_>) -> arbitrary::Result<Self> {
        Ok(Self {
            tile: u.arbitrary()?,
        })
    }
}

impl MvtRoundtripInput {
    pub fn fuzz_roundtrip(self) {
        let Ok(canonical) = decode_proto_tile(&self.tile) else {
            return;
        };
        let normalized = mvt_roundtrip(canonical).expect("canonical MVT tile should re-encode");
        let again =
            mvt_roundtrip(normalized.clone()).expect("normalized MVT tile should re-encode");
        assert_eq!(normalized, again, "MVT round trip is not idempotent");

        let bytes = normalized
            .encode_ref()
            .expect("normalized tile should encode");
        let proto =
            Tile::decode_from_slice(&bytes).expect("written bytes should be valid protobuf");
        assert_eq!(
            proto.encode_to_vec(),
            bytes,
            "writer output differs from the generated protobuf encoder"
        );
        assert_eq!(
            stream_tile(&normalized).expect("normalized tile should stream"),
            bytes,
            "streamed tile differs from the encoded one"
        );
    }
}

/// Writes `tile` with the streaming builder API, each layer on its own and the chunks concatenated.
fn stream_tile(tile: &MvtTile) -> MvtResult<Vec<u8>> {
    let mut chunks = Vec::new();
    for layer in &tile.layers {
        let mut layer_bld = MvtLayerBuilder::new(layer.name.as_str())?;
        layer_bld.extent(layer.extent);
        for feature in &layer.features {
            layer_bld = stream_feature(layer_bld, feature)?;
        }
        chunks.push(layer_bld.encode());
    }
    Ok(chunks.concat())
}

fn stream_feature(
    layer: MvtLayerBuilder,
    feature: &fast_mvt::MvtFeature,
) -> MvtResult<MvtLayerBuilder> {
    let mut bld = match &feature.geometry {
        MvtGeometry::Point(point) => {
            let mut bld = layer.feature_of(MvtGeomType::Point);
            bld.points([point.0])?;
            bld
        }
        MvtGeometry::MultiPoint(points) => {
            let mut bld = layer.feature_of(MvtGeomType::Point);
            bld.points(points.0.iter().map(|point| point.0))?;
            bld
        }
        MvtGeometry::LineString(line) => {
            let mut bld = layer.feature_of(MvtGeomType::LineString);
            bld.line(line.0.iter().copied())?;
            bld
        }
        MvtGeometry::MultiLineString(lines) => {
            let mut bld = layer.feature_of(MvtGeomType::LineString);
            for line in &lines.0 {
                bld.line(line.0.iter().copied())?;
            }
            bld
        }
        MvtGeometry::Polygon(polygon) => {
            let mut bld = layer.feature_of(MvtGeomType::Polygon);
            stream_polygon(&mut bld, polygon)?;
            bld
        }
        MvtGeometry::MultiPolygon(polygons) => {
            let mut bld = layer.feature_of(MvtGeomType::Polygon);
            for polygon in &polygons.0 {
                stream_polygon(&mut bld, polygon)?;
            }
            bld
        }
        // The reader produces nothing else
        other => unreachable!("unexpected decoded geometry {other:?}"),
    };
    bld.id(feature.id);
    for (key, value) in &feature.properties {
        bld.tag_ref(key, value.into())?;
    }
    Ok(bld.end())
}

fn stream_polygon(
    bld: &mut fast_mvt::MvtFeatureBuilder,
    polygon: &fast_mvt::MvtPolygon,
) -> MvtResult<()> {
    bld.ring(polygon.exterior().0.iter().copied(), true)?;
    for ring in polygon.interiors() {
        bld.ring(ring.0.iter().copied(), false)?;
    }
    Ok(())
}

fn decode_proto_tile(tile: &Tile) -> MvtResult<MvtTile> {
    let bytes = tile.encode_to_vec();
    MvtReaderRef::new(&bytes).and_then(|reader| reader.to_tile())
}

fn mvt_roundtrip(tile: MvtTile) -> MvtResult<MvtTile> {
    let bytes = tile.encode()?;
    MvtReaderRef::new(&bytes).and_then(|reader| reader.to_tile())
}

impl std::fmt::Debug for MvtRoundtripInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "MvtRoundtripInput {{\n\ttile: {:#?}\n}}", self.tile)
    }
}
