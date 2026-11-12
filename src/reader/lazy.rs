//! A reader that decodes a tile only as far as it is read.
//!
//! [`MvtLazyReader`] finds a tile's layers without decoding them.
//! [`MvtLazyLayer`] decodes one layer's keys and values and finds its
//! features. [`MvtLazyFeature`] decodes a feature when it is read, and its
//! tags and geometry commands are read from the tile's bytes as they are
//! iterated. Reading a tile allocates only each decoded layer's tables, so
//! it suits a reader that reads some layers, or each feature once, where
//! [`MvtReaderRef`](crate::MvtReaderRef) decodes everything up front.
//!

use buffa::Enumeration as _;
use buffa::MessageView as _;
use buffa::encoding::{Tag, WireType, check_wire_type, decode_varint, skip_field};
use buffa::types::{borrow_bytes, borrow_str, decode_int32, decode_uint32, decode_uint64};
use usize_cast::IntoUsize as _;

use super::property::value_ref;
use crate::generated::vector_tile::tile as proto_tile;
use crate::geom_reader::decode_geometry;
use crate::{DEFAULT_EXTENT, MvtError, MvtGeometry, MvtResult, MvtValueRef};

/// A tile whose layers are found, but not decoded.
#[derive(Clone, Debug, Default)]
pub struct MvtLazyReader<'a> {
    layers: Vec<MvtLazyLayerEntry<'a>>,
}

/// A layer of an [`MvtLazyReader`]: its name, and its message's bytes.
#[derive(Copy, Clone, Debug)]
pub struct MvtLazyLayerEntry<'a> {
    name: &'a str,
    data: &'a [u8],
}

impl<'a> MvtLazyReader<'a> {
    /// Finds the layers of the tile `data`, and their names.
    ///
    /// # Errors
    ///
    /// The tile isn't a valid protobuf message, a layer isn't, or a layer
    /// has no name.
    pub fn new(data: &'a [u8]) -> MvtResult<Self> {
        let mut layers = Vec::new();
        let mut buf = data;
        while !buf.is_empty() {
            let tag = Tag::decode(&mut buf)?;
            if tag.field_number() == 3 {
                check_wire_type(tag, WireType::LengthDelimited)?;
                let layer = borrow_bytes(&mut buf)?;
                layers.push(MvtLazyLayerEntry {
                    name: layer_name(layer)?,
                    data: layer,
                });
            } else {
                skip_field(tag, &mut buf)?;
            }
        }
        Ok(Self { layers })
    }

    /// The layers, in tile order.
    pub fn layers(&self) -> impl ExactSizeIterator<Item = MvtLazyLayerEntry<'a>> + '_ {
        self.layers.iter().copied()
    }

    #[must_use]
    pub fn layer_count(&self) -> usize {
        self.layers.len()
    }
}

/// The last name of a layer message, which must not be empty.
fn layer_name(mut buf: &[u8]) -> MvtResult<&str> {
    let mut name = "";
    while !buf.is_empty() {
        let tag = Tag::decode(&mut buf)?;
        if tag.field_number() == 1 {
            check_wire_type(tag, WireType::LengthDelimited)?;
            name = borrow_str(&mut buf)?;
        } else {
            skip_field(tag, &mut buf)?;
        }
    }
    if name.is_empty() {
        return Err(MvtError::MissingLayerName);
    }
    Ok(name)
}

impl<'a> MvtLazyLayerEntry<'a> {
    #[must_use]
    pub fn name(self) -> &'a str {
        self.name
    }

    /// The layer message's bytes, without the tile field's tag and length.
    #[must_use]
    pub fn data(self) -> &'a [u8] {
        self.data
    }

    /// Decodes the layer, as [`MvtLazyLayer::decode`] does.
    pub fn decode(self) -> MvtResult<MvtLazyLayer<'a>> {
        MvtLazyLayer::decode(self.data)
    }
}

/// A layer whose keys and values are decoded, and whose features are found
/// but not decoded.
#[derive(Clone, Debug)]
pub struct MvtLazyLayer<'a> {
    name: &'a str,
    version: u32,
    extent: Option<u32>,
    keys: Vec<&'a str>,
    values: Vec<MvtValueRef<'a>>,
    features: Vec<&'a [u8]>,
}

impl<'a> MvtLazyLayer<'a> {
    /// Decodes a layer message: the bytes of a tile's layer field, without
    /// its tag and length.
    ///
    /// # Errors
    ///
    /// The layer isn't a valid protobuf message, has no name, or has a
    /// version other than 1, 2 or 3.
    pub fn decode(data: &'a [u8]) -> MvtResult<Self> {
        let mut layer = Self {
            name: "",
            version: 0,
            extent: None,
            keys: Vec::new(),
            values: Vec::new(),
            features: Vec::new(),
        };
        let mut buf = data;
        while !buf.is_empty() {
            let tag = Tag::decode(&mut buf)?;
            match tag.field_number() {
                1 => {
                    check_wire_type(tag, WireType::LengthDelimited)?;
                    layer.name = borrow_str(&mut buf)?;
                }
                2 => {
                    check_wire_type(tag, WireType::LengthDelimited)?;
                    layer.features.push(borrow_bytes(&mut buf)?);
                }
                3 => {
                    check_wire_type(tag, WireType::LengthDelimited)?;
                    layer.keys.push(borrow_str(&mut buf)?);
                }
                4 => {
                    check_wire_type(tag, WireType::LengthDelimited)?;
                    let value = proto_tile::ValueView::decode_view(borrow_bytes(&mut buf)?)?;
                    layer.values.push(value_ref(&value));
                }
                5 => {
                    check_wire_type(tag, WireType::Varint)?;
                    layer.extent = Some(decode_uint32(&mut buf)?);
                }
                15 => {
                    check_wire_type(tag, WireType::Varint)?;
                    layer.version = decode_uint32(&mut buf)?;
                }
                _ => skip_field(tag, &mut buf)?,
            }
        }
        if layer.name.is_empty() {
            return Err(MvtError::MissingLayerName);
        }
        if !(1..=3).contains(&layer.version) {
            return Err(MvtError::UnsupportedVersion {
                layer: layer.name.to_string(),
                version: layer.version,
            });
        }
        Ok(layer)
    }

    #[must_use]
    pub fn name(&self) -> &'a str {
        self.name
    }

    #[must_use]
    pub fn version(&self) -> u32 {
        self.version
    }

    #[must_use]
    pub fn extent(&self) -> u32 {
        self.extent.unwrap_or(DEFAULT_EXTENT.get())
    }

    #[must_use]
    pub fn keys(&self) -> &[&'a str] {
        &self.keys
    }

    #[must_use]
    pub fn values(&self) -> &[MvtValueRef<'a>] {
        &self.values
    }

    #[must_use]
    pub fn feature_count(&self) -> usize {
        self.features.len()
    }

    /// Decodes the feature at `index`, `None` past the end.
    #[must_use]
    pub fn feature(&self, index: usize) -> Option<MvtResult<MvtLazyFeature<'_, 'a>>> {
        self.features
            .get(index)
            .map(|data| MvtLazyFeature::decode(self, data))
    }

    /// Decodes each feature as the iterator reaches it.
    pub fn features(
        &self,
    ) -> impl ExactSizeIterator<Item = MvtResult<MvtLazyFeature<'_, 'a>>> + '_ {
        self.features
            .iter()
            .map(|data| MvtLazyFeature::decode(self, data))
    }
}

/// A feature of an [`MvtLazyLayer`], whose tags and geometry commands are
/// read as they are iterated.
#[derive(Clone, Debug)]
pub struct MvtLazyFeature<'l, 'a> {
    layer: &'l MvtLazyLayer<'a>,
    id: Option<u64>,
    geom_type: Option<i32>,
    tags: MvtPackedIter<'a>,
    geometry: MvtPackedIter<'a>,
}

/// The occurrences of a repeated `uint32` field in a message.
#[derive(Default)]
struct RepeatedField<'a> {
    /// The message from the field's first occurrence on.
    from_first: &'a [u8],
    /// The first occurrence's values, if it is packed.
    packed: &'a [u8],
    occurrences: usize,
    unpacked: bool,
}

impl<'a> RepeatedField<'a> {
    /// Reads past an occurrence of the field, whose tag `tag` starts `at`.
    fn read(&mut self, at: &'a [u8], tag: Tag, buf: &mut &'a [u8]) -> MvtResult<()> {
        if self.occurrences == 0 {
            self.from_first = at;
        }
        self.occurrences += 1;
        if tag.wire_type() == WireType::Varint {
            self.unpacked = true;
            decode_varint(buf)?;
        } else {
            check_wire_type(tag, WireType::LengthDelimited)?;
            let packed = borrow_bytes(buf)?;
            if self.occurrences == 1 {
                self.packed = packed;
            }
        }
        Ok(())
    }

    fn values(&self, field_number: u32) -> MvtPackedIter<'a> {
        if self.occurrences == 1 && !self.unpacked {
            MvtPackedIter {
                field_number,
                packed: self.packed,
                rest: &[],
            }
        } else {
            MvtPackedIter {
                field_number,
                packed: &[],
                rest: self.from_first,
            }
        }
    }
}

impl<'l, 'a> MvtLazyFeature<'l, 'a> {
    fn decode(layer: &'l MvtLazyLayer<'a>, data: &'a [u8]) -> MvtResult<Self> {
        let mut id = None;
        let mut geom_type = None;
        let mut tags = RepeatedField::default();
        let mut geometry = RepeatedField::default();
        let mut buf = data;
        while !buf.is_empty() {
            let at = buf;
            let tag = Tag::decode(&mut buf)?;
            match tag.field_number() {
                1 => {
                    check_wire_type(tag, WireType::Varint)?;
                    id = Some(decode_uint64(&mut buf)?);
                }
                2 => tags.read(at, tag, &mut buf)?,
                3 => {
                    check_wire_type(tag, WireType::Varint)?;
                    geom_type = Some(decode_int32(&mut buf)?);
                }
                4 => geometry.read(at, tag, &mut buf)?,
                _ => skip_field(tag, &mut buf)?,
            }
        }
        Ok(Self {
            layer,
            id,
            geom_type,
            tags: tags.values(2),
            geometry: geometry.values(4),
        })
    }

    #[must_use]
    pub fn id(&self) -> Option<u64> {
        self.id
    }

    /// The geometry type, `None` for none or an unknown one.
    #[must_use]
    pub fn geom_type(&self) -> Option<proto_tile::GeomType> {
        self.geom_type.and_then(proto_tile::GeomType::from_i32)
    }

    /// The geometry type's value as the tile has it.
    #[must_use]
    pub fn geom_type_value(&self) -> Option<i32> {
        self.geom_type
    }

    #[must_use]
    pub fn has_properties(&self) -> bool {
        self.tags().next().is_some()
    }

    /// The tags: key and value indices, alternating.
    #[must_use]
    pub fn tags(&self) -> MvtPackedIter<'a> {
        self.tags.clone()
    }

    #[must_use]
    pub fn properties(&self) -> MvtLazyPropertyIter<'l, 'a> {
        MvtLazyPropertyIter {
            keys: &self.layer.keys,
            values: &self.layer.values,
            tags: self.tags(),
        }
    }

    #[must_use]
    pub fn geometry_commands(&self) -> MvtPackedIter<'a> {
        self.geometry.clone()
    }

    /// The geometry, as [`MvtFeatureRef::geometry`](crate::MvtFeatureRef::geometry) decodes it.
    ///
    /// # Errors
    ///
    /// The geometry commands are malformed.
    pub fn geometry(&self) -> MvtResult<MvtGeometry> {
        let commands = self.geometry_commands().collect::<MvtResult<Vec<_>>>()?;
        decode_geometry(self.geom_type(), &commands)
    }
}

/// The values of a feature's repeated `uint32` field, decoded from the
/// tile's bytes as they are iterated. A field that occurs more than once,
/// packed or not, gives the values of each occurrence in turn. After an
/// error, it ends.
#[derive(Clone, Debug)]
pub struct MvtPackedIter<'a> {
    field_number: u32,
    /// The packed values still to read.
    packed: &'a [u8],
    /// The rest of the message, to search for more occurrences of the field.
    rest: &'a [u8],
}

impl MvtPackedIter<'_> {
    #[cold]
    fn next_occurrence(&mut self) -> Option<MvtResult<u32>> {
        while !self.rest.is_empty() {
            let result = (|| {
                let tag = Tag::decode(&mut self.rest)?;
                if tag.field_number() != self.field_number {
                    skip_field(tag, &mut self.rest)?;
                    return Ok(None);
                }
                if tag.wire_type() == WireType::Varint {
                    return Ok(Some(decode_varint(&mut self.rest)?));
                }
                check_wire_type(tag, WireType::LengthDelimited)?;
                self.packed = borrow_bytes(&mut self.rest)?;
                Ok(None)
            })();
            match result {
                Ok(Some(value)) => return Some(Ok(truncate(value))),
                Ok(None) if !self.packed.is_empty() => return self.next(),
                Ok(None) => {}
                Err(error) => return Some(Err(self.fail(error))),
            }
        }
        None
    }

    fn fail(&mut self, error: buffa::DecodeError) -> MvtError {
        self.packed = &[];
        self.rest = &[];
        error.into()
    }
}

/// `uint32`: the low 32 bits of the varint.
#[expect(clippy::cast_possible_truncation)]
fn truncate(value: u64) -> u32 {
    value as u32
}

impl Iterator for MvtPackedIter<'_> {
    type Item = MvtResult<u32>;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        if self.packed.is_empty() {
            return self.next_occurrence();
        }
        Some(match decode_varint(&mut self.packed) {
            Ok(value) => Ok(truncate(value)),
            Err(error) => Err(self.fail(error)),
        })
    }
}

/// The properties of an [`MvtLazyFeature`]: each tag's key and value.
#[derive(Clone, Debug)]
pub struct MvtLazyPropertyIter<'l, 'a> {
    keys: &'l [&'a str],
    values: &'l [MvtValueRef<'a>],
    tags: MvtPackedIter<'a>,
}

impl<'a> Iterator for MvtLazyPropertyIter<'_, 'a> {
    type Item = MvtResult<(&'a str, MvtValueRef<'a>)>;

    fn next(&mut self) -> Option<Self::Item> {
        let key_idx = match self.tags.next()? {
            Ok(key) => key,
            Err(error) => return Some(Err(error)),
        };
        let value_idx = match self.tags.next() {
            Some(Ok(value)) => value,
            Some(Err(error)) => return Some(Err(error)),
            None => return Some(Err(MvtError::InvalidTagsLength(1))),
        };
        let Some(&key) = self.keys.get(key_idx.into_usize()) else {
            return Some(Err(MvtError::InvalidKeyIndex(key_idx)));
        };
        let Some(&value) = self.values.get(value_idx.into_usize()) else {
            return Some(Err(MvtError::InvalidValueIndex(value_idx)));
        };
        Some(Ok((key, value)))
    }
}

#[cfg(test)]
mod tests {
    use buffa::Message as _;

    use super::*;
    use crate::MvtReaderRef;
    use crate::generated::vector_tile::Tile;

    fn layer(name: &str, features: Vec<proto_tile::Feature>) -> proto_tile::Layer {
        proto_tile::Layer {
            version: 2,
            name: name.into(),
            keys: vec!["k".into(), "n".into()],
            values: vec![
                proto_tile::Value {
                    string_value: Some("v".into()),
                    ..Default::default()
                },
                proto_tile::Value {
                    double_value: Some(1.5),
                    ..Default::default()
                },
            ],
            features,
            ..Default::default()
        }
    }

    fn feature(id: u64, tags: Vec<u32>, geometry: Vec<u32>) -> proto_tile::Feature {
        proto_tile::Feature {
            id: Some(id),
            tags,
            r#type: Some(proto_tile::GeomType::Polygon),
            geometry,
        }
    }

    #[test]
    fn reads_as_the_eager_reader_does() {
        let square = vec![9, 0, 0, 26, 20, 0, 0, 20, 19, 0, 15];
        let bytes = Tile {
            layers: vec![
                layer("a", vec![feature(1, vec![0, 0, 1, 1], square.clone())]),
                layer(
                    "b",
                    vec![
                        feature(2, vec![1, 1], square.clone()),
                        feature(3, vec![], vec![]),
                    ],
                ),
            ],
        }
        .encode_to_vec();
        let eager = MvtReaderRef::new(&bytes).unwrap();
        let lazy = MvtLazyReader::new(&bytes).unwrap();
        assert_eq!(lazy.layer_count(), eager.layer_count());
        for (entry, eager) in lazy.layers().zip(eager.layers()) {
            assert_eq!(entry.name(), eager.name());
            let layer = entry.decode().unwrap();
            assert_eq!(layer.name(), eager.name());
            assert_eq!(layer.version(), eager.version());
            assert_eq!(layer.extent(), eager.extent());
            assert_eq!(layer.keys(), eager.keys());
            assert!(layer.values().iter().copied().eq(eager.values()));
            assert_eq!(layer.feature_count(), eager.feature_count());
            for (lazy, eager) in layer.features().zip(eager.features()) {
                let lazy = lazy.unwrap();
                assert_eq!(lazy.id(), eager.id());
                assert_eq!(lazy.geom_type(), eager.geom_type());
                assert_eq!(
                    lazy.tags().collect::<MvtResult<Vec<_>>>().unwrap(),
                    eager.tags()
                );
                assert_eq!(
                    lazy.geometry_commands()
                        .collect::<MvtResult<Vec<_>>>()
                        .unwrap(),
                    eager.geometry_commands()
                );
                assert_eq!(
                    lazy.properties().collect::<MvtResult<Vec<_>>>().unwrap(),
                    eager.properties_vec().unwrap()
                );
                assert_eq!(lazy.geometry().ok(), eager.geometry().ok());
            }
        }
        let reader = MvtLazyReader::new(&bytes).unwrap();
        let b = reader
            .layers()
            .find(|entry| entry.name() == "b")
            .unwrap()
            .decode()
            .unwrap();
        assert_eq!(b.feature(1).unwrap().unwrap().id(), Some(3));
        assert!(b.feature(2).is_none());
    }

    #[test]
    fn reports_malformed_tags_and_layers() {
        let bytes = Tile {
            layers: vec![layer("a", vec![feature(1, vec![0, 0, 5, 0, 1], vec![])])],
        }
        .encode_to_vec();
        let reader = MvtLazyReader::new(&bytes).unwrap();
        let decoded = reader.layers().next().unwrap().decode().unwrap();
        let feature = decoded.feature(0).unwrap().unwrap();
        let properties: Vec<_> = feature
            .properties()
            .map(|p| p.map_err(|e| e.to_string()))
            .collect();
        assert_eq!(properties[0], Ok(("k", MvtValueRef::String("v"))));
        assert_eq!(properties[1], Err("invalid key index 5".to_string()));
        assert_eq!(
            properties[2],
            Err("invalid feature tags length: 1".to_string())
        );

        let mut version_4 = layer("a", vec![]);
        version_4.version = 4;
        let bytes = Tile {
            layers: vec![version_4],
        }
        .encode_to_vec();
        let entry = MvtLazyReader::new(&bytes).unwrap().layers().next().unwrap();
        assert!(matches!(
            entry.decode(),
            Err(MvtError::UnsupportedVersion { version: 4, .. })
        ));
    }
}
