use buffa::Message as _;
use fast_mvt::proto::Tile;
use fast_mvt::{MvtReaderRef, MvtResult, MvtTile};

/// Fuzz input exercising `Tile protobuf -> MvtTile -> bytes -> MvtTile`.
///
/// The first public round trip is normalizing: unsupported geometry streams,
/// invalid layer metadata, and null-valued tags may be rejected or canonicalized.
/// Once normalized, subsequent round trips must be fixpoints.
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
    }
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
