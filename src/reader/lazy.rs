//! A reader that decodes a tile only as far as it is read.
//!
//! [`MvtLazyReader`] finds a tile's layers without decoding them.
//! [`MvtLazyLayer`] decodes one layer's keys and values and finds its
//! features. [`MvtLazyFeature`] decodes a feature when it is read, checking
//! its tags and geometry commands, which are decoded from the tile's bytes as
//! they are iterated. Reading a tile allocates only each decoded layer's tables, so
//! it suits a reader that reads some layers, or each feature once, where
//! [`MvtReaderRef`](crate::MvtReaderRef) decodes everything up front.
//!

use buffa::Enumeration as _;
use buffa::encoding::{Tag, WireType, check_wire_type, skip_field};
use buffa::types::{borrow_bytes, borrow_str};

use super::property::MvtPropertyIter;
use super::wire::{FeatureFields, LayerItem, read_feature, read_layer};
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
        let (mut keys, mut values, mut features) = (Vec::new(), Vec::new(), Vec::new());
        let header = read_layer(data, |item| {
            match item {
                LayerItem::Feature(feature) => features.push(feature),
                LayerItem::Key(key) => keys.push(key),
                LayerItem::Value(value) => values.push(value),
            }
            Ok(())
        })?;
        let layer = Self {
            name: header.name,
            version: header.version,
            extent: header.extent,
            keys,
            values,
            features,
        };
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
/// checked, and decoded as they are iterated.
#[derive(Clone, Debug)]
pub struct MvtLazyFeature<'l, 'a> {
    layer: &'l MvtLazyLayer<'a>,
    fields: FeatureFields<'a>,
    /// Packed fields written in several records, joined here; empty for nearly every feature.
    joined: Vec<u8>,
}

impl<'l, 'a> MvtLazyFeature<'l, 'a> {
    fn decode(layer: &'l MvtLazyLayer<'a>, data: &'a [u8]) -> MvtResult<Self> {
        let mut joined = Vec::new();
        let fields = read_feature(data, &mut joined)?;
        Ok(Self {
            layer,
            fields,
            joined,
        })
    }

    #[must_use]
    pub fn id(&self) -> Option<u64> {
        self.fields.id
    }

    /// The geometry type, `None` for none or an unknown one.
    #[must_use]
    pub fn geom_type(&self) -> Option<proto_tile::GeomType> {
        self.fields.geom_type
    }

    /// The geometry type's value, `None` for none or an unknown one.
    #[must_use]
    pub fn geom_type_value(&self) -> Option<i32> {
        self.fields.geom_type.map(|geom_type| geom_type.to_i32())
    }

    #[must_use]
    pub fn has_properties(&self) -> bool {
        !self.fields.tags.is_empty()
    }

    /// The tags: key and value indices, alternating.
    pub fn tags(&self) -> impl ExactSizeIterator<Item = u32> + Clone + '_ {
        self.fields.tags.iter(&self.joined)
    }

    #[must_use]
    pub fn properties(&self) -> MvtPropertyIter<'_, 'a> {
        MvtPropertyIter::new(
            &self.layer.keys,
            &self.layer.values,
            self.fields.tags.iter(&self.joined),
        )
    }

    pub fn geometry_commands(&self) -> impl ExactSizeIterator<Item = u32> + Clone + '_ {
        self.fields.geometry.iter(&self.joined)
    }

    /// The geometry, as [`MvtFeatureRef::geometry`](crate::MvtFeatureRef::geometry) decodes it.
    ///
    /// # Errors
    ///
    /// The geometry commands are malformed.
    pub fn geometry(&self) -> MvtResult<MvtGeometry> {
        decode_geometry(self.geom_type(), self.geometry_commands())
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
                assert!(lazy.tags().eq(eager.tags()));
                assert!(lazy.geometry_commands().eq(eager.geometry_commands()));
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

        // A feature's packed fields are checked when it is decoded, as the eager reader checks
        // them when it reads the tile: here an unterminated varint in the tags.
        let feature = [0x12, 0x01, 0x80];
        let mut layer_bytes = vec![0x0a, 0x01, b'l', 0x78, 0x02, 0x12, 0x03];
        layer_bytes.extend(feature);
        let mut bytes = vec![0x1a, u8::try_from(layer_bytes.len()).unwrap()];
        bytes.extend(layer_bytes);
        assert!(MvtReaderRef::new(&bytes).is_err());
        let entry = MvtLazyReader::new(&bytes).unwrap().layers().next().unwrap();
        assert!(entry.decode().unwrap().feature(0).unwrap().is_err());

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
