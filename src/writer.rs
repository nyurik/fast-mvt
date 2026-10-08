//! Writes MVT protobuf bytes as features are added, field for field what the generated message
//! code would encode, without building a message per feature.

use std::collections::HashMap as StdHashMap;

use buffa::Enumeration as _;
use buffa::encoding::varint_len;
use buffa::types::{
    encode_uint32, int32_encoded_len, int64_encoded_len, put_bool_field, put_double_field,
    put_float_field, put_int32_field, put_int64_field, put_len_delimited_header, put_sint64_field,
    put_string_field, put_uint32_field, put_uint64_field, sint64_encoded_len, string_encoded_len,
    uint32_encoded_len, uint64_encoded_len,
};

use crate::generated::vector_tile::tile::GeomType;
use crate::geom_writer::{GeometryBuf, u32_index};
use crate::{
    DEFAULT_EXTENT, MvtCoord, MvtError, MvtExtent, MvtGeomType, MvtGeometry, MvtResult, MvtTile,
    MvtValue, MvtValueRef,
};

/// Randomly seeded, so tile data can not be crafted to collide, but much cheaper than the default.
type HashMap<K, V> = StdHashMap<K, V, foldhash::fast::RandomState>;

/// MVT spec version written into every layer.
const LAYER_VERSION: u32 = 2;

#[derive(Debug, Default)]
pub struct MvtTileBuilder {
    /// The encoded `Tile.layers` fields so far.
    buf: Vec<u8>,
}

impl MvtTileBuilder {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn layer(self, name: impl Into<String>) -> MvtResult<MvtLayerBuilder> {
        let name = name.into();
        if name.is_empty() {
            return Err(MvtError::MissingLayerName);
        }
        Ok(MvtLayerBuilder::with_tile(self, name))
    }

    #[must_use]
    pub fn encode(self) -> Vec<u8> {
        self.buf
    }

    #[must_use]
    pub fn encoded_len(&self) -> usize {
        self.buf.len()
    }
}

pub(crate) fn encode_tile(tile: &MvtTile) -> MvtResult<Vec<u8>> {
    let mut tile_bld = MvtTileBuilder::new();
    for layer in &tile.layers {
        let mut layer_bld = tile_bld.layer(layer.name.as_str())?;
        layer_bld.extent(layer.extent);
        for feature in &layer.features {
            let mut feature_bld = layer_bld.feature(&feature.geometry)?;
            feature_bld.id(feature.id);
            for (key, value) in &feature.properties {
                feature_bld.tag_ref(key, value.into())?;
            }
            layer_bld = feature_bld.end();
        }
        tile_bld = layer_bld.end();
    }
    Ok(tile_bld.encode())
}

#[derive(Debug)]
pub struct MvtLayerBuilder {
    tile: MvtTileBuilder,
    name: String,
    extent: u32,
    /// The encoded `Layer.features` fields so far.
    features: Vec<u8>,
    num_features: usize,
    keys: HashMap<Box<str>, u32>,
    values: ValueIndex,
    /// The feature being built, kept between features so its buffers are reused.
    feature: FeatureBuf,
}

#[derive(Debug, Default)]
struct FeatureBuf {
    id: Option<u64>,
    geom_type: GeomType,
    tags: Vec<u32>,
    geometry: GeometryBuf,
}

impl MvtLayerBuilder {
    /// Create a standalone layer builder that is not attached to a tile.
    ///
    /// This is also a convenient entry point for building a layer directly: add
    /// features and tags as usual, then either [`end`](Self::end) it into a tile
    /// or [`encode`](Self::encode) it on its own.
    ///
    /// Finishing with [`encode`](Self::encode) yields a framed layer chunk.
    /// Independently built layer buffers (for example, one per thread) can be
    /// concatenated to form a complete tile — see the crate-level parallel
    /// encoding example. Returns [`MvtError::MissingLayerName`] if `name` is empty.
    pub fn new(name: impl Into<String>) -> MvtResult<Self> {
        MvtTileBuilder::new().layer(name)
    }

    fn with_tile(tile: MvtTileBuilder, name: String) -> Self {
        Self {
            tile,
            name,
            extent: DEFAULT_EXTENT.get(),
            features: Vec::new(),
            num_features: 0,
            keys: HashMap::default(),
            values: ValueIndex::default(),
            feature: FeatureBuf::default(),
        }
    }

    pub fn extent(&mut self, extent: MvtExtent) -> &mut Self {
        self.extent = extent.get();
        self
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub fn num_features(&self) -> usize {
        self.num_features
    }

    pub fn feature(mut self, geometry: &MvtGeometry) -> MvtResult<MvtFeatureBuilder> {
        self.feature.reset();
        self.feature.geom_type = self.feature.geometry.push_geometry(geometry)?;
        Ok(MvtFeatureBuilder { layer: self })
    }

    /// Starts a feature whose geometry is added piece by piece, with [`MvtFeatureBuilder::points`],
    /// [`MvtFeatureBuilder::line`] or [`MvtFeatureBuilder::ring`], straight from its coordinates.
    #[must_use = "call .end() to commit the feature to the layer"]
    pub fn feature_of(mut self, geom_type: MvtGeomType) -> MvtFeatureBuilder {
        self.feature.reset();
        self.feature.geom_type = match geom_type {
            MvtGeomType::Point => GeomType::Point,
            MvtGeomType::LineString => GeomType::Linestring,
            MvtGeomType::Polygon => GeomType::Polygon,
        };
        MvtFeatureBuilder { layer: self }
    }

    #[must_use]
    pub fn end(self) -> MvtTileBuilder {
        let Self {
            mut tile,
            name,
            extent,
            features,
            keys,
            values,
            ..
        } = self;
        let mut keys: Vec<_> = keys.into_iter().collect();
        keys.sort_unstable_by_key(|(_, idx)| *idx);
        let values = values.into_ordered();

        let len = 1
            + string_encoded_len(&name)
            + features.len()
            + keys
                .iter()
                .map(|(key, _)| 1 + string_encoded_len(key))
                .sum::<usize>()
            + values
                .iter()
                .map(|value| {
                    let len = value.encoded_len();
                    1 + varint_len(len as u64) + len
                })
                .sum::<usize>()
            + 1
            + uint32_encoded_len(extent)
            + 1
            + uint32_encoded_len(LAYER_VERSION);

        let buf = &mut tile.buf;
        buf.reserve(len + 10);
        put_len_delimited_header(3, len as u64, buf);
        put_string_field(1, &name, buf);
        buf.extend_from_slice(&features);
        for (key, _) in &keys {
            put_string_field(3, key, buf);
        }
        for value in &values {
            put_len_delimited_header(4, value.encoded_len() as u64, buf);
            value.write(buf);
        }
        put_uint32_field(5, extent, buf);
        put_uint32_field(15, LAYER_VERSION, buf);
        tile
    }

    /// Commit this layer and start a new one.
    ///
    /// This is a shortcut for `self.end().layer(name)` that keeps the chain on
    /// layer builders without exposing the intermediate [`MvtTileBuilder`].
    /// Returns [`MvtError::MissingLayerName`] if `name` is empty.
    pub fn layer(self, name: impl Into<String>) -> MvtResult<Self> {
        self.end().layer(name)
    }

    /// Commit this layer and encode the tile built so far.
    ///
    /// For a builder created with [`MvtLayerBuilder::new`], the parent tile is
    /// empty, so this encodes exactly this one layer as a framed chunk — several
    /// such buffers can be concatenated (for example with `buffers.concat()`)
    /// into a multi-layer tile. For a builder obtained from
    /// [`MvtTileBuilder::layer`], the result also includes any previously
    /// committed layers, making it equivalent to `self.end().encode()`.
    #[must_use]
    pub fn encode(self) -> Vec<u8> {
        self.end().encode()
    }

    /// Appends the current feature as a `Layer.features` field.
    fn write_feature(&mut self) {
        let FeatureBuf {
            id,
            geom_type,
            tags,
            geometry,
        } = &self.feature;
        let tags_len = packed_len(tags);
        let geometry_len = packed_len(&geometry.data);
        let len = id.map_or(0, |id| 1 + uint64_encoded_len(id))
            + packed_field_len(tags_len)
            + 1
            + int32_encoded_len(geom_type.to_i32())
            + packed_field_len(geometry_len);

        let buf = &mut self.features;
        put_len_delimited_header(2, len as u64, buf);
        if let Some(id) = *id {
            put_uint64_field(1, id, buf);
        }
        put_packed(2, tags, tags_len, buf);
        put_int32_field(3, geom_type.to_i32(), buf);
        put_packed(4, &geometry.data, geometry_len, buf);
        self.num_features += 1;
    }
}

impl FeatureBuf {
    fn reset(&mut self) {
        self.id = None;
        self.tags.clear();
        self.geometry.clear();
    }
}

#[derive(Debug)]
#[must_use = "call .end() to commit the feature to the layer"]
pub struct MvtFeatureBuilder {
    /// The feature itself is the layer's [`FeatureBuf`].
    layer: MvtLayerBuilder,
}

impl MvtFeatureBuilder {
    pub fn id(&mut self, id: Option<u64>) -> &mut Self {
        self.layer.feature.id = id;
        self
    }

    pub fn tag(
        &mut self,
        key: impl AsRef<str>,
        value: impl Into<MvtValue>,
    ) -> MvtResult<&mut Self> {
        self.tag_ref(key.as_ref(), (&value.into()).into())
    }

    /// Like [`Self::tag`], without allocating for keys and values the layer already has.
    pub fn tag_ref(&mut self, key: &str, value: MvtValueRef<'_>) -> MvtResult<&mut Self> {
        let layer = &mut self.layer;
        if let Some(value_idx) = layer.values.index(value)? {
            let key_idx = if let Some(&idx) = layer.keys.get(key) {
                idx
            } else {
                let idx = u32_index(layer.keys.len())?;
                layer.keys.insert(key.into(), idx);
                idx
            };
            layer.feature.tags.extend([key_idx, value_idx]);
        }
        Ok(self)
    }

    pub fn tag_string(
        &mut self,
        key: impl AsRef<str>,
        value: impl Into<String>,
    ) -> MvtResult<&mut Self> {
        self.tag(key, MvtValue::String(value.into()))
    }

    pub fn tag_float(&mut self, key: impl AsRef<str>, value: f32) -> MvtResult<&mut Self> {
        self.tag_ref(key.as_ref(), MvtValueRef::Float(value))
    }

    pub fn tag_double(&mut self, key: impl AsRef<str>, value: f64) -> MvtResult<&mut Self> {
        self.tag_ref(key.as_ref(), MvtValueRef::Double(value))
    }

    pub fn tag_int(&mut self, key: impl AsRef<str>, value: i64) -> MvtResult<&mut Self> {
        self.tag_ref(key.as_ref(), MvtValueRef::Int(value))
    }

    pub fn tag_uint(&mut self, key: impl AsRef<str>, value: u64) -> MvtResult<&mut Self> {
        self.tag_ref(key.as_ref(), MvtValueRef::UInt(value))
    }

    pub fn tag_sint(&mut self, key: impl AsRef<str>, value: i64) -> MvtResult<&mut Self> {
        self.tag_ref(key.as_ref(), MvtValueRef::SInt(value))
    }

    /// Add an integer tag using the smallest MVT encoding for `value`.
    ///
    /// See [`MvtValue::auto_int`] for how the encoding is chosen.
    pub fn tag_auto_int(
        &mut self,
        key: impl AsRef<str>,
        value: impl Into<i64>,
    ) -> MvtResult<&mut Self> {
        self.tag(key, MvtValue::auto_int(value))
    }

    pub fn tag_bool(&mut self, key: impl AsRef<str>, value: bool) -> MvtResult<&mut Self> {
        self.tag_ref(key.as_ref(), MvtValueRef::Bool(value))
    }

    /// Adds points to a [`MvtGeomType::Point`] feature, as a single `MoveTo` command. A feature
    /// with no points, such as an empty multi-point, has no geometry.
    pub fn points(&mut self, coords: impl IntoIterator<Item = MvtCoord>) -> MvtResult<&mut Self> {
        self.geometry(GeomType::Point)?.points(coords)?;
        Ok(self)
    }

    /// Adds a line to a [`MvtGeomType::LineString`] feature.
    pub fn line(&mut self, coords: impl IntoIterator<Item = MvtCoord>) -> MvtResult<&mut Self> {
        self.geometry(GeomType::Linestring)?.line(coords)?;
        Ok(self)
    }

    /// Adds a ring to a [`MvtGeomType::Polygon`] feature: a polygon is its exterior ring followed
    /// by its interior ones. The ring may be open or closed, and is rewound to the MVT winding of
    /// its role if needed.
    pub fn ring(
        &mut self,
        coords: impl IntoIterator<Item = MvtCoord>,
        exterior: bool,
    ) -> MvtResult<&mut Self> {
        self.geometry(GeomType::Polygon)?.ring(coords, exterior)?;
        Ok(self)
    }

    fn geometry(&mut self, expected: GeomType) -> MvtResult<&mut GeometryBuf> {
        let feature = &mut self.layer.feature;
        if feature.geom_type == expected {
            Ok(&mut feature.geometry)
        } else {
            Err(MvtError::InvalidGeometry)
        }
    }

    #[must_use]
    pub fn num_tags(&self) -> usize {
        self.layer.feature.tags.len() / 2
    }

    #[must_use]
    pub fn end(mut self) -> MvtLayerBuilder {
        self.layer.write_feature();
        self.layer
    }
}

/// A layer's values, numbered in the order they first appear. Strings are looked up by `&str`, so
/// only a new value allocates.
#[derive(Debug, Default)]
struct ValueIndex {
    strings: HashMap<Box<str>, u32>,
    scalars: HashMap<Scalar, u32>,
}

/// A non-string value, with floats compared by their bits as [`MvtValue`] does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Scalar {
    Float(u32),
    Double(u64),
    Int(i64),
    UInt(u64),
    SInt(i64),
    Bool(bool),
}

impl ValueIndex {
    /// The index of `value`, or `None` for a null, which MVT cannot store.
    fn index(&mut self, value: MvtValueRef<'_>) -> MvtResult<Option<u32>> {
        let next = u32_index(self.strings.len() + self.scalars.len())?;
        let scalar = match value {
            MvtValueRef::String(value) => {
                return Ok(Some(match self.strings.get(value) {
                    Some(&idx) => idx,
                    None => *self.strings.entry(value.into()).or_insert(next),
                }));
            }
            MvtValueRef::Float(value) => Scalar::Float(value.to_bits()),
            MvtValueRef::Double(value) => Scalar::Double(value.to_bits()),
            MvtValueRef::Int(value) => Scalar::Int(value),
            MvtValueRef::UInt(value) => Scalar::UInt(value),
            MvtValueRef::SInt(value) => Scalar::SInt(value),
            MvtValueRef::Bool(value) => Scalar::Bool(value),
            MvtValueRef::Null => return Ok(None),
        };
        Ok(Some(*self.scalars.entry(scalar).or_insert(next)))
    }

    fn into_ordered(self) -> Vec<LayerValue> {
        let mut values: Vec<_> = (self.strings.into_iter())
            .map(|(value, idx)| (idx, LayerValue::String(value)))
            .chain((self.scalars.into_iter()).map(|(value, idx)| (idx, LayerValue::Scalar(value))))
            .collect();
        values.sort_unstable_by_key(|(idx, _)| *idx);
        values.into_iter().map(|(_, value)| value).collect()
    }
}

/// A `Layer.values` entry, as a `Value` message with one field set.
enum LayerValue {
    String(Box<str>),
    Scalar(Scalar),
}

impl LayerValue {
    fn encoded_len(&self) -> usize {
        1 + match self {
            Self::String(value) => string_encoded_len(value),
            Self::Scalar(Scalar::Float(_)) => 4,
            Self::Scalar(Scalar::Double(_)) => 8,
            Self::Scalar(Scalar::Int(value)) => int64_encoded_len(*value),
            Self::Scalar(Scalar::UInt(value)) => uint64_encoded_len(*value),
            Self::Scalar(Scalar::SInt(value)) => sint64_encoded_len(*value),
            Self::Scalar(Scalar::Bool(_)) => 1,
        }
    }

    fn write(&self, buf: &mut Vec<u8>) {
        match *self {
            Self::String(ref value) => put_string_field(1, value, buf),
            Self::Scalar(Scalar::Float(bits)) => put_float_field(2, f32::from_bits(bits), buf),
            Self::Scalar(Scalar::Double(bits)) => put_double_field(3, f64::from_bits(bits), buf),
            Self::Scalar(Scalar::Int(value)) => put_int64_field(4, value, buf),
            Self::Scalar(Scalar::UInt(value)) => put_uint64_field(5, value, buf),
            Self::Scalar(Scalar::SInt(value)) => put_sint64_field(6, value, buf),
            Self::Scalar(Scalar::Bool(value)) => put_bool_field(7, value, buf),
        }
    }
}

fn packed_len(values: &[u32]) -> usize {
    values.iter().map(|&v| uint32_encoded_len(v)).sum()
}

/// The size of a packed field holding `len` bytes, which is not written at all when empty.
fn packed_field_len(len: usize) -> usize {
    if len == 0 {
        0
    } else {
        1 + varint_len(len as u64) + len
    }
}

fn put_packed(field: u32, values: &[u32], len: usize, buf: &mut Vec<u8>) {
    if !values.is_empty() {
        put_len_delimited_header(field, len as u64, buf);
        for &value in values {
            encode_uint32(value, buf);
        }
    }
}

#[cfg(test)]
mod tests {
    #![expect(clippy::panic_in_result_fn)]

    use buffa::Message as _;
    use geo_types::point;

    use super::*;
    use crate::generated::vector_tile::Tile;
    use crate::proto::Value;

    #[test]
    fn layer_builder_deduplicates_keys_and_values() {
        let layer = MvtTileBuilder::new().layer("layer").unwrap();
        let mut feature = layer
            .feature(&MvtGeometry::Point(point! { x: 1, y: 2 }))
            .unwrap();
        feature.tag("foo", MvtValue::String("bar".into())).unwrap();
        feature.tag("foo", MvtValue::String("baz".into())).unwrap();
        feature.tag("bar", MvtValue::String("bar".into())).unwrap();
        feature.tag("n", MvtValue::Int(1)).unwrap();
        feature.tag("n", MvtValue::SInt(1)).unwrap();
        feature.tag("f", MvtValue::Float(f32::NAN)).unwrap();
        feature.tag("f", MvtValue::Float(f32::NAN)).unwrap();

        assert_eq!(
            feature.layer.feature.tags,
            vec![0, 0, 0, 1, 1, 0, 2, 2, 2, 3, 3, 4, 3, 4]
        );
    }

    #[test]
    fn encode_appends_and_validates_tile_metadata() {
        let tile = MvtTileBuilder::new();
        let layer = tile.layer("layer").unwrap();
        let mut feature = layer
            .feature(&MvtGeometry::Point(point! { x: 1, y: 2 }))
            .unwrap();
        feature.id(Some(1));
        feature.tag("skip", MvtValue::Null).unwrap();
        let layer = feature.end();
        let bytes = layer.end().encode();
        let proto = Tile::decode_from_slice(&bytes).unwrap();
        assert_eq!(proto.layers[0].keys, Vec::<String>::new());
        assert_eq!(proto.layers[0].features[0].tags, Vec::<u32>::new());

        let tile = MvtTileBuilder::new();
        let layer = tile.layer("layer").unwrap();
        let mut feature = layer
            .feature(&MvtGeometry::Point(point! { x: 1, y: 2 }))
            .unwrap();
        feature.id(Some(1));
        let layer = feature.end();
        let tile = layer.end();
        let mut out = vec![0xaa];
        out.extend_from_slice(&tile.encode());
        assert_eq!(out[0], 0xaa);

        let tile = MvtTileBuilder::new();
        let tile = tile.layer("same").unwrap().end();
        let tile = tile.layer("same").unwrap().end();
        assert_ne!(tile.encode(), Vec::<u8>::new());
    }

    #[test]
    fn encode_ref_matches_owned_encode() {
        let mut feature = crate::MvtFeature::new(MvtGeometry::Point(point! { x: 1, y: 2 }));
        feature.set_id(7);
        feature.add_tag_string("name", "Example");
        feature.add_tag_bool("visible", true);

        let mut layer = crate::MvtLayer::new("places", DEFAULT_EXTENT);
        layer.add_feature(feature);

        let mut tile = MvtTile::new();
        tile.add_layer(layer);

        assert_eq!(tile.clone().encode().unwrap(), tile.encode_ref().unwrap());
    }

    #[test]
    #[cfg(feature = "reader")]
    fn standalone_layer_encode_matches_tile_path_and_concatenates() {
        use crate::reader::MvtReaderRef;

        let build = |name| -> MvtResult<Vec<u8>> {
            let mut feature =
                MvtLayerBuilder::new(name)?.feature(&MvtGeometry::Point(point! { x: 1, y: 2 }))?;
            feature.tag("k", MvtValue::UInt(1))?;
            Ok(feature.end().encode())
        };

        // A standalone layer buffer equals the same layer built via the tile path.
        let via_tile = MvtTileBuilder::new()
            .layer("roads")
            .unwrap()
            .feature(&MvtGeometry::Point(point! { x: 1, y: 2 }))
            .unwrap();
        let mut via_tile = via_tile;
        via_tile.tag("k", MvtValue::UInt(1)).unwrap();
        let via_tile = via_tile.end().end().encode();
        assert_eq!(build("roads").unwrap(), via_tile);

        // Concatenated layer buffers form a valid multi-layer tile.
        let tile = [build("roads").unwrap(), build("water").unwrap()].concat();
        let reader = MvtReaderRef::new(&tile).unwrap();
        let names: Vec<_> = reader.layers().map(|l| l.name().to_string()).collect();
        assert_eq!(names, ["roads", "water"]);
    }

    #[test]
    #[cfg(feature = "reader")]
    fn layer_builder_chains_to_next_layer() -> MvtResult<()> {
        use crate::reader::MvtReaderRef;

        // Chaining `.layer(..)` keeps the builder on the layer without exposing
        // the tile, and produces the same tile as the explicit tile path.
        let chained = MvtLayerBuilder::new("roads")?
            .feature(&MvtGeometry::Point(point! { x: 1, y: 2 }))?
            .end()
            .layer("water")?
            .feature(&MvtGeometry::Point(point! { x: 3, y: 4 }))?
            .end()
            .encode();

        let reader = MvtReaderRef::new(&chained)?;
        let names: Vec<_> = reader.layers().map(|l| l.name().to_string()).collect();
        assert_eq!(names, ["roads", "water"]);
        Ok(())
    }

    #[test]
    fn layer_builder_chain_rejects_empty_name() {
        let layer = MvtLayerBuilder::new("roads").unwrap();
        assert!(matches!(layer.layer(""), Err(MvtError::MissingLayerName)));
    }

    #[test]
    fn standalone_layer_builder_rejects_empty_name() {
        assert!(matches!(
            MvtLayerBuilder::new(""),
            Err(MvtError::MissingLayerName)
        ));
    }

    #[test]
    fn layer_builder_rejects_empty_name() {
        assert!(matches!(
            MvtTileBuilder::new().layer(""),
            Err(MvtError::MissingLayerName)
        ));
    }

    #[test]
    fn builder_encoded_len_matches_encoded_bytes() {
        let builder = MvtTileBuilder::new()
            .layer("l")
            .unwrap()
            .feature(&MvtGeometry::Point(point! { x: 1, y: 2 }))
            .unwrap()
            .end()
            .end();
        let len = builder.encoded_len();
        assert_eq!(len, builder.encode().len());
    }

    #[test]
    #[cfg(feature = "reader")]
    fn tag_auto_int_round_trips_through_reader() {
        use crate::MvtValueRef;
        use crate::reader::MvtReaderRef;

        let tile = MvtTileBuilder::new();
        let layer = tile.layer("l").unwrap();
        let mut feature = layer
            .feature(&MvtGeometry::Point(point! { x: 1, y: 2 }))
            .unwrap();
        feature.tag_auto_int("pos", 100_i32).unwrap();
        feature.tag_auto_int("neg", -100_i16).unwrap();
        feature.tag_auto_int("zero", 0_i64).unwrap();
        let bytes = feature.end().end().encode();

        let reader = MvtReaderRef::new(&bytes).unwrap();
        let layer = reader.layers().next().unwrap();
        let feature = layer.features().next().unwrap();
        let props = feature.properties_vec().unwrap();

        // Non-negative -> UInt, negative -> SInt.
        assert_eq!(props[0].0, "pos");
        assert_eq!(props[0].1, MvtValueRef::UInt(100));
        assert_eq!(props[1].0, "neg");
        assert_eq!(props[1].1, MvtValueRef::SInt(-100));
        assert_eq!(props[2].0, "zero");
        assert_eq!(props[2].1, MvtValueRef::UInt(0));
    }

    #[test]
    fn auto_int_is_never_larger_than_int_or_sint() {
        for v in [
            0_i64,
            1,
            63,
            64,
            127,
            128,
            -1,
            -64,
            -100,
            i64::MIN,
            i64::MAX,
        ] {
            let auto = encoded_value(&MvtValue::auto_int(v)).len();
            let int = encoded_value(&MvtValue::Int(v)).len();
            let sint = encoded_value(&MvtValue::SInt(v)).len();
            assert!(auto <= int, "v={v}: auto {auto} > int {int}");
            assert!(auto <= sint, "v={v}: auto {auto} > sint {sint}");
        }
    }

    /// The same features written from geometries and piece by piece, with owned and borrowed tags.
    #[test]
    fn streamed_features_encode_like_geometries() -> MvtResult<()> {
        use geo_types::{LineString, MultiLineString, MultiPoint, MultiPolygon, Polygon};

        let ring = |pts: &[(i32, i32)]| LineString::from(pts.to_vec());
        let square = [(0, 0), (10, 0), (10, 10), (0, 10)];
        let hole = [(2, 2), (4, 2), (4, 4), (2, 4)];
        let geometries = [
            MvtGeometry::MultiPoint(MultiPoint(vec![
                point! { x: 1, y: 2 },
                point! { x: 3, y: 1 },
            ])),
            MvtGeometry::MultiLineString(MultiLineString(vec![
                ring(&[(0, 0), (5, 5)]),
                ring(&[(9, 9), (1, 1), (4, 4)]),
            ])),
            MvtGeometry::MultiPolygon(MultiPolygon(vec![
                Polygon::new(ring(&square), vec![ring(&hole)]),
                Polygon::new(ring(&[(20, 20), (30, 20), (30, 30), (20, 20)]), vec![]),
            ])),
        ];

        let mut by_geometry = MvtLayerBuilder::new("l")?;
        for geometry in &geometries {
            let mut feature = by_geometry.feature(geometry)?;
            feature
                .id(Some(3))
                .tag("name", "a")?
                .tag("n", MvtValue::SInt(-2))?;
            by_geometry = feature.end();
        }

        let tags = |feature: &mut MvtFeatureBuilder| -> MvtResult<()> {
            feature
                .id(Some(3))
                .tag_ref("name", MvtValueRef::String("a"))?
                .tag_ref("n", MvtValueRef::SInt(-2))?;
            Ok(())
        };
        let mut streamed = MvtLayerBuilder::new("l")?;
        let coords = |pts: &[(i32, i32)]| {
            pts.iter()
                .map(|&(x, y)| MvtCoord { x, y })
                .collect::<Vec<_>>()
        };
        let mut feature = streamed.feature_of(MvtGeomType::Point);
        feature.points(coords(&[(1, 2), (3, 1)]))?;
        tags(&mut feature)?;
        streamed = feature.end();
        let mut feature = streamed.feature_of(MvtGeomType::LineString);
        feature
            .line(coords(&[(0, 0), (5, 5)]))?
            .line(coords(&[(9, 9), (1, 1), (4, 4)]))?;
        tags(&mut feature)?;
        streamed = feature.end();
        // Rings may be open or closed, and are rewound to the winding of their role.
        let mut feature = streamed.feature_of(MvtGeomType::Polygon);
        let mut reversed = coords(&square);
        reversed.reverse();
        feature.ring(reversed, true)?.ring(coords(&hole), false)?;
        feature.ring(coords(&[(20, 20), (30, 20), (30, 30), (20, 20)]), true)?;
        tags(&mut feature)?;
        streamed = feature.end();

        assert_eq!(streamed.encode(), by_geometry.encode());
        Ok(())
    }

    #[test]
    fn streamed_geometry_must_match_the_feature_type() {
        let coord = MvtCoord { x: 1, y: 1 };
        let mut feature = MvtLayerBuilder::new("l")
            .unwrap()
            .feature_of(MvtGeomType::LineString);
        assert!(matches!(
            feature.points([coord]),
            Err(MvtError::InvalidGeometry)
        ));
        assert!(matches!(
            feature.ring([coord], true),
            Err(MvtError::InvalidGeometry)
        ));
        assert!(matches!(feature.line([]), Err(MvtError::InvalidGeometry)));
        feature.line([coord]).unwrap();
    }

    /// The `Value` message the layer writes for `value`.
    fn encoded_value(value: &MvtValue) -> Vec<u8> {
        let mut values = ValueIndex::default();
        values.index(value.into()).unwrap();
        let mut buf = Vec::new();
        for value in values.into_ordered() {
            value.write(&mut buf);
            assert_eq!(buf.len(), value.encoded_len());
        }
        buf
    }

    #[test]
    fn values_encode_like_the_generated_message() {
        let cases = [
            (
                MvtValue::String("x".into()),
                Value::default().with_string_value("x"),
            ),
            (MvtValue::Float(1.0), Value::default().with_float_value(1.0)),
            (
                MvtValue::Double(2.0),
                Value::default().with_double_value(2.0),
            ),
            (MvtValue::Int(-3), Value::default().with_int_value(-3)),
            (MvtValue::UInt(4), Value::default().with_uint_value(4)),
            (MvtValue::SInt(-5), Value::default().with_sint_value(-5)),
            (MvtValue::Bool(true), Value::default().with_bool_value(true)),
        ];
        for (value, proto) in cases {
            assert_eq!(encoded_value(&value), proto.encode_to_vec(), "{value:?}");
        }
        assert_eq!(encoded_value(&MvtValue::Null), b"");
    }
}
