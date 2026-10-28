use buffa::{DecodeError, DecodeOptions, Message as _};
use fast_mvt::proto::{Tile, Value};
use fast_mvt::{DEFAULT_EXTENT, MvtError, MvtReaderRef, MvtValueRef};

/// Fuzz input exercising the reader on raw bytes: an arbitrary tile, encoded, then edited byte by
/// byte, so that most inputs are almost valid.
///
/// What the reader accepts must read the same as the generated protobuf decoder reads it, field
/// for field, and what that decoder rejects, the reader must reject too.
pub struct MvtReaderInput {
    pub bytes: Vec<u8>,
}

impl arbitrary::Arbitrary<'_> for MvtReaderInput {
    fn arbitrary(u: &mut arbitrary::Unstructured<'_>) -> arbitrary::Result<Self> {
        let tile: Tile = u.arbitrary()?;
        let mut bytes = tile.encode_to_vec();
        for _ in 0..u.int_in_range(0..=4)? {
            if bytes.is_empty() {
                break;
            }
            let pos = u.choose_index(bytes.len())?;
            match u.int_in_range(0..=3)? {
                0 => bytes[pos] = u.arbitrary()?,
                1 => bytes.insert(pos, u.arbitrary()?),
                2 => {
                    bytes.remove(pos);
                }
                _ => bytes.truncate(pos),
            }
        }
        Ok(Self { bytes })
    }
}

impl MvtReaderInput {
    pub fn fuzz_reader(&self) {
        // The reader limits the memory it decodes into; converting to the owned form did not.
        let proto = DecodeOptions::new()
            .with_element_memory_limit(usize::MAX)
            .decode_from_slice::<Tile>(&self.bytes);
        let reader = match MvtReaderRef::new(&self.bytes) {
            Ok(reader) => reader,
            Err(MvtError::Decode(DecodeError::ElementMemoryLimitExceeded)) => return,
            Err(MvtError::Decode(error)) => {
                assert!(
                    proto.is_err(),
                    "the reader rejects what the generated decoder reads: {error}"
                );
                return;
            }
            // Layer names and versions are checked on top of the protobuf.
            Err(_) => return,
        };
        let proto = proto.expect("the generated decoder rejects what the reader reads");

        assert_eq!(reader.layer_count(), proto.layers.len());
        for (layer, expected) in reader.layers().zip(&proto.layers) {
            assert_eq!(layer.name(), expected.name);
            assert_eq!(layer.version(), expected.version);
            assert_eq!(
                layer.extent(),
                expected.extent.unwrap_or(DEFAULT_EXTENT.get())
            );
            assert!(layer.keys().iter().eq(&expected.keys));
            let values: Vec<_> = layer.values().map(|value| format!("{value:?}")).collect();
            let expected_values: Vec<_> = (expected.values.iter())
                .map(|value| format!("{:?}", value_ref(value)))
                .collect();
            assert_eq!(values, expected_values);
            assert_eq!(layer.feature_count(), expected.features.len());
            for (feature, expected) in layer.features().zip(&expected.features) {
                assert_eq!(feature.id(), expected.id);
                assert_eq!(feature.geom_type(), expected.r#type);
                // Read the packed fields lazily first, then as the slices decoded for the tile.
                let _ = feature.geometry();
                assert_eq!(
                    feature.properties().count(),
                    expected.tags.len().div_ceil(2)
                );
                assert_eq!(feature.tags(), expected.tags);
                assert_eq!(feature.geometry_commands(), expected.geometry);
            }
        }
    }
}

/// The value the reader gives for `value`: its first field set, in the order they are declared.
fn value_ref(value: &Value) -> MvtValueRef<'_> {
    if let Some(value) = &value.string_value {
        MvtValueRef::String(value)
    } else if let Some(value) = value.float_value {
        MvtValueRef::Float(value)
    } else if let Some(value) = value.double_value {
        MvtValueRef::Double(value)
    } else if let Some(value) = value.int_value {
        MvtValueRef::Int(value)
    } else if let Some(value) = value.uint_value {
        MvtValueRef::UInt(value)
    } else if let Some(value) = value.sint_value {
        MvtValueRef::SInt(value)
    } else if let Some(value) = value.bool_value {
        MvtValueRef::Bool(value)
    } else {
        MvtValueRef::Null
    }
}

impl std::fmt::Debug for MvtReaderInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "MvtReaderInput {{ bytes: {:02x?} }}", self.bytes)
    }
}
