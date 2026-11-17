//! Writes MVT protobuf bytes as features are added, field for field what the generated message
//! code would encode, without building a message per feature.

use std::hash::{BuildHasher as _, Hasher as _};

use buffa::Enumeration as _;
use buffa::encoding::{Tag, WireType, encode_varint, varint_len};
use buffa::types::{
    int32_encoded_len, put_int32_field, put_len_delimited_header, put_string_field,
    put_uint32_field, put_uint64_field, uint64_encoded_len,
};
use hashbrown::HashTable;
use usize_cast::IntoUsize as _;

use crate::generated::vector_tile::tile::GeomType;
use crate::geom_writer::{GeometryBuf, u32_index};
use crate::{
    DEFAULT_EXTENT, MvtCoord, MvtError, MvtExtent, MvtGeomType, MvtGeometry, MvtResult, MvtTile,
    MvtValue, MvtValueRef,
};

/// MVT spec version written into every layer.
const LAYER_VERSION: u32 = 2;

/// The longest a varint can be, kept free for a layer's length until the layer ends.
const MAX_VARINT_LEN: usize = 10;

#[derive(Debug, Default)]
pub struct MvtTileBuilder {
    /// The encoded `Tile.layers` fields so far.
    buf: Vec<u8>,
    /// Keys the next layer starts with, in this order, whether or not a feature uses them.
    next_keys: Vec<Box<str>>,
    /// The state of the last layer, kept so the next one reuses its tables and buffers.
    spare: Option<Box<LayerState>>,
}

impl MvtTileBuilder {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Makes the next layer list `keys` first, in this order, **even those no feature uses**
    pub fn preset_keys<'k>(&mut self, keys: impl IntoIterator<Item = &'k str>) -> &mut Self {
        self.next_keys = keys.into_iter().map(Box::from).collect();
        self
    }

    pub fn layer(self, name: impl Into<String>) -> MvtResult<MvtLayerBuilder> {
        let name = name.into();
        if name.is_empty() {
            return Err(MvtError::MissingLayerName);
        }
        MvtLayerBuilder::with_tile(self, name)
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
    /// Boxed, so that handing the builder from feature to feature moves a pointer.
    state: Box<LayerState>,
}

#[derive(Debug, Default)]
struct LayerState {
    name: String,
    extent: u32,
    /// Where the layer's length goes in the tile buffer, which holds the layer as it is written.
    start: usize,
    num_features: usize,
    keys: Interner,
    values: Interner,
    /// For each key, the index of the value it last had, which it most likely has again.
    last_values: Vec<u32>,
    /// The feature being built, kept between features so its buffers are reused.
    feature: FeatureBuf,
}

#[derive(Debug, Default)]
struct FeatureBuf {
    id: Option<u64>,
    geom_type: GeomType,
    tags: Vec<u32>,
    /// The tags of the previous feature, whose keys the next feature most likely repeats in order.
    last_tags: Vec<u32>,
    geometry: GeometryBuf,
    /// The varints of the packed tags, as written.
    packed_tags: Vec<u8>,
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

    fn with_tile(mut tile: MvtTileBuilder, name: String) -> MvtResult<Self> {
        let mut state = tile.spare.take().unwrap_or_default();
        Tag::new(3, WireType::LengthDelimited).encode(&mut tile.buf);
        state.start = tile.buf.len();
        tile.buf.extend_from_slice(&[0; MAX_VARINT_LEN]);
        put_string_field(1, &name, &mut tile.buf);
        state.name = name;
        state.extent = DEFAULT_EXTENT.get();
        for key in std::mem::take(&mut tile.next_keys) {
            state.keys.index(KEY, key.as_bytes(), None)?;
        }
        Ok(Self { tile, state })
    }

    pub fn extent(&mut self, extent: MvtExtent) -> &mut Self {
        self.state.extent = extent.get();
        self
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.state.name
    }

    #[must_use]
    pub fn num_features(&self) -> usize {
        self.state.num_features
    }

    pub fn feature(mut self, geometry: &MvtGeometry) -> MvtResult<MvtFeatureBuilder> {
        let feature = &mut self.state.feature;
        feature.reset();
        feature.geom_type = feature.geometry.push_geometry(geometry)?;
        Ok(MvtFeatureBuilder { layer: self })
    }

    /// Starts a feature whose geometry is added piece by piece, with [`MvtFeatureBuilder::points`],
    /// [`MvtFeatureBuilder::line`] or [`MvtFeatureBuilder::ring`], straight from its coordinates.
    #[must_use = "call .end() to commit the feature to the layer"]
    pub fn feature_of(mut self, geom_type: MvtGeomType) -> MvtFeatureBuilder {
        self.state.feature.reset();
        self.state.feature.geom_type = match geom_type {
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
            mut state,
        } = self;
        let buf = &mut tile.buf;
        buf.extend_from_slice(&state.keys.encoded);
        buf.extend_from_slice(&state.values.encoded);
        put_uint32_field(5, state.extent, buf);
        put_uint32_field(15, LAYER_VERSION, buf);

        // The layer's length goes into the room left for it, and the layer moves up to follow it.
        let body = state.start + MAX_VARINT_LEN;
        let mut len = (buf.len() - body) as u64;
        let mut end = state.start;
        while len >= 0x80 {
            buf[end] = len.to_le_bytes()[0] | 0x80;
            len >>= 7;
            end += 1;
        }
        buf[end] = len.to_le_bytes()[0];
        end += 1;
        buf.copy_within(body.., end);
        buf.truncate(buf.len() - (body - end));

        state.clear();
        tile.spare = Some(state);
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
            packed_tags,
            ..
        } = &mut self.state.feature;
        // The tags are packed first, as their length comes before them; the geometry is held as
        // its packed varints already.
        pack_varints(tags, packed_tags);
        let len = id.map_or(0, |id| 1 + uint64_encoded_len(id))
            + packed_field_len(packed_tags.len())
            + 1
            + int32_encoded_len(geom_type.to_i32())
            + packed_field_len(geometry.data.len());

        let buf = &mut self.tile.buf;
        buf.reserve(1 + varint_len(len as u64) + len);
        put_len_delimited_header(2, len as u64, buf);
        if let Some(id) = *id {
            put_uint64_field(1, id, buf);
        }
        put_packed(2, packed_tags, buf);
        put_int32_field(3, geom_type.to_i32(), buf);
        put_packed(4, &geometry.data, buf);
        self.state.num_features += 1;
    }
}

impl LayerState {
    /// Empties the layer for reuse, keeping the capacity of its tables and buffers.
    fn clear(&mut self) {
        self.name.clear();
        self.num_features = 0;
        self.keys.clear();
        self.values.clear();
        self.last_values.clear();
        self.feature.reset();
        self.feature.last_tags.clear();
    }
}

impl FeatureBuf {
    fn reset(&mut self) {
        self.id = None;
        std::mem::swap(&mut self.tags, &mut self.last_tags);
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
        self.layer.state.feature.id = id;
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
        let mut scratch = [0; MAX_VARINT_LEN];
        let Some((kind, payload)) = value_payload(value, &mut scratch) else {
            return Ok(self);
        };
        let layer = &mut *self.layer.state;
        let feature = &mut layer.feature;
        let predicted = feature.last_tags.get(feature.tags.len()).copied();
        let key_idx = layer.keys.index(KEY, key.as_bytes(), predicted)?;
        let slot = key_idx.into_usize();
        let predicted = layer.last_values.get(slot).copied();
        let value_idx = layer.values.index(kind, payload, predicted)?;
        if let Some(last) = layer.last_values.get_mut(slot) {
            *last = value_idx;
        } else {
            layer.last_values.resize(slot, u32::MAX);
            layer.last_values.push(value_idx);
        }
        feature.tags.extend([key_idx, value_idx]);
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
        let feature = &mut self.layer.state.feature;
        if feature.geom_type == expected {
            Ok(&mut feature.geometry)
        } else {
            Err(MvtError::InvalidGeometry)
        }
    }

    #[must_use]
    pub fn num_tags(&self) -> usize {
        self.layer.state.feature.tags.len() / 2
    }

    #[must_use]
    pub fn end(mut self) -> MvtLayerBuilder {
        self.layer.write_feature();
        self.layer
    }
}

/// The kind of a key in an [`Interner`]; values use their `Value` field number, from 1 to 7.
const KEY: u8 = 0;

/// A layer's keys or values, numbered in the order they first appear. Each is kept once, as the
/// layer field it is written as, and is looked up by comparing against those bytes, so no key or
/// value allocates, and the layer is written by copying them.
#[derive(Debug, Default)]
struct Interner {
    /// The `Layer.keys` or `Layer.values` fields, in index order.
    encoded: Vec<u8>,
    /// By index, the kind of each entry and where its payload is in `encoded`.
    entries: Vec<Entry>,
    /// The entry indices, by the hash of their kind and payload.
    table: HashTable<u32>,
    /// Randomly seeded, so tile data can not be crafted to collide, but much cheaper than the default.
    hasher: foldhash::fast::RandomState,
}

/// An entry of an [`Interner`]: its kind, and its payload as `encoded[start..end]`.
#[derive(Debug, Clone, Copy)]
struct Entry {
    kind: u8,
    start: u32,
    end: u32,
}

impl Interner {
    /// The index of the entry of `kind` with `payload`, adding it if new. The `predicted` index is
    /// checked first, which saves hashing the payload when it is right.
    #[expect(
        clippy::inline_always,
        reason = "plain `#[inline]` left one of its two call sites in `tag_ref` out of line, at 4% of encoding"
    )]
    #[inline(always)]
    fn index(&mut self, kind: u8, payload: &[u8], predicted: Option<u32>) -> MvtResult<u32> {
        if let Some(idx) = predicted {
            if self.matches(idx, kind, payload) {
                return Ok(idx);
            }
        }
        let hash = hash_entry(&self.hasher, kind, payload);
        if let Some(&idx) = self
            .table
            .find(hash, |&idx| self.matches(idx, kind, payload))
        {
            return Ok(idx);
        }
        self.insert(hash, kind, payload)
    }

    #[inline]
    fn matches(&self, idx: u32, kind: u8, payload: &[u8]) -> bool {
        self.entries.get(idx.into_usize()).is_some_and(|entry| {
            entry.kind == kind
                && (self
                    .encoded
                    .get(entry.start.into_usize()..entry.end.into_usize()))
                .is_some_and(|known| bytes_eq(known, payload))
        })
    }

    /// Adds a new entry. Kept out of line, so that lookups, far more frequent, stay small.
    #[inline(never)]
    fn insert(&mut self, hash: u64, kind: u8, payload: &[u8]) -> MvtResult<u32> {
        let idx = u32_index(self.entries.len())?;
        write_entry(kind, payload, &mut self.encoded);
        let end = u32_index(self.encoded.len())?;
        let start = u32_index(self.encoded.len() - payload.len())?;
        self.entries.push(Entry { kind, start, end });
        let Self {
            encoded,
            entries,
            table,
            hasher,
        } = self;
        table.insert_unique(hash, idx, |&idx| {
            let entry = entries[idx.into_usize()];
            let payload = &encoded[entry.start.into_usize()..entry.end.into_usize()];
            hash_entry(hasher, entry.kind, payload)
        });
        Ok(idx)
    }

    /// Empties the interner for reuse, keeping its capacity.
    fn clear(&mut self) {
        self.encoded.clear();
        self.entries.clear();
        self.table.clear();
    }
}

/// Whether `a` and `b` are equal. Up to 16 bytes, which most keys and values of a layer are, they
/// are compared as two overlapping words from either end, which is cheaper than calling `memcmp`.
#[expect(
    clippy::inline_always,
    reason = "called in every lookup, for a few instructions"
)]
#[inline(always)]
fn bytes_eq(a: &[u8], b: &[u8]) -> bool {
    let len = a.len();
    if len != b.len() {
        false
    } else if len > 16 {
        a == b
    } else if len >= 8 {
        a.first_chunk::<8>() == b.first_chunk::<8>() && a.last_chunk::<8>() == b.last_chunk::<8>()
    } else if len >= 4 {
        a.first_chunk::<4>() == b.first_chunk::<4>() && a.last_chunk::<4>() == b.last_chunk::<4>()
    } else {
        // The first, middle and last bytes are all the bytes of up to three.
        a.first() == b.first() && a.get(len / 2) == b.get(len / 2) && a.last() == b.last()
    }
}

#[inline]
fn hash_entry(hasher: &foldhash::fast::RandomState, kind: u8, payload: &[u8]) -> u64 {
    let mut hasher = hasher.build_hasher();
    hasher.write(payload);
    // Payloads of different kinds rarely match, so the kind needs no hashing of its own.
    hasher.finish() ^ u64::from(kind)
}

/// Appends the layer field for an entry: a `Layer.keys` string, or a `Layer.values` message
/// holding the one `Value` field `kind` with `payload`.
fn write_entry(kind: u8, payload: &[u8], buf: &mut Vec<u8>) {
    let len = payload.len() as u64;
    if kind == KEY {
        put_len_delimited_header(3, len, buf);
    } else if kind == 1 {
        put_len_delimited_header(4, 1 + varint_len(len) as u64 + len, buf);
        put_len_delimited_header(1, len, buf);
    } else {
        let wire_type = match kind {
            2 => WireType::Fixed32,
            3 => WireType::Fixed64,
            _ => WireType::Varint,
        };
        put_len_delimited_header(4, 1 + len, buf);
        Tag::new(u32::from(kind), wire_type).encode(buf);
    }
    buf.extend_from_slice(payload);
}

/// The kind and payload of a value: its `Value` field number, and the bytes of that field after
/// its tag, without the length of a string. Floats compare by their bits, as in [`MvtValue`]. A
/// null has none, as MVT cannot store it.
fn value_payload<'a>(
    value: MvtValueRef<'a>,
    scratch: &'a mut [u8; MAX_VARINT_LEN],
) -> Option<(u8, &'a [u8])> {
    let (kind, varint) = match value {
        MvtValueRef::String(value) => return Some((1, value.as_bytes())),
        MvtValueRef::Float(value) => {
            scratch[..4].copy_from_slice(&value.to_bits().to_le_bytes());
            return Some((2, &scratch[..4]));
        }
        MvtValueRef::Double(value) => {
            scratch[..8].copy_from_slice(&value.to_bits().to_le_bytes());
            return Some((3, &scratch[..8]));
        }
        MvtValueRef::Int(value) => (4, value.cast_unsigned()),
        MvtValueRef::UInt(value) => (5, value),
        MvtValueRef::SInt(value) => (6, ((value << 1) ^ (value >> 63)).cast_unsigned()),
        MvtValueRef::Bool(value) => (7, u64::from(value)),
        MvtValueRef::Null => return None,
    };
    let mut sink = &mut scratch[..];
    encode_varint(varint, &mut sink);
    let len = MAX_VARINT_LEN - sink.len();
    Some((kind, &scratch[..len]))
}

/// Replaces the contents of `out` with `values` as varints.
fn pack_varints(values: &[u32], out: &mut Vec<u8>) {
    out.clear();
    for &value in values {
        encode_varint(u64::from(value), out);
    }
}

/// The size of a packed field holding `len` bytes, which is not written at all when empty.
fn packed_field_len(len: usize) -> usize {
    if len == 0 {
        0
    } else {
        1 + varint_len(len as u64) + len
    }
}

fn put_packed(field: u32, packed: &[u8], buf: &mut Vec<u8>) {
    if !packed.is_empty() {
        put_len_delimited_header(field, packed.len() as u64, buf);
        buf.extend_from_slice(packed);
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
            feature.layer.state.feature.tags,
            vec![0, 0, 0, 1, 1, 0, 2, 2, 2, 3, 3, 4, 3, 4]
        );
    }

    #[test]
    #[cfg(feature = "reader")]
    fn preset_keys_start_the_next_layer_only() {
        let mut tile = MvtTileBuilder::new();
        tile.preset_keys(["id", "name", "kind", "name"]);
        let mut feature = tile
            .layer("first")
            .unwrap()
            .feature(&MvtGeometry::Point(point! { x: 1, y: 2 }))
            .unwrap();
        feature.tag("extra", MvtValue::Int(1)).unwrap();
        feature.tag("name", MvtValue::String("a".into())).unwrap();
        let mut feature = feature
            .end()
            .layer("second")
            .unwrap()
            .feature(&MvtGeometry::Point(point! { x: 3, y: 4 }))
            .unwrap();
        feature.tag("name", MvtValue::String("b".into())).unwrap();
        let bytes = feature.end().encode();

        let reader = crate::MvtReaderRef::new(&bytes).unwrap();
        insta::assert_debug_snapshot!(reader, @r#"
        layer: 0
          name: first
          version: 2
          extent: 4096
          key table: ["id", "name", "kind", "extra"]
          feature: 0
            id: (none)
            geometry: POINT(1,2)
            properties:
              extra = 1 (int)
              name = "a"
        =============================================================
        layer: 1
          name: second
          version: 2
          extent: 4096
          feature: 0
            id: (none)
            geometry: POINT(3,4)
            properties:
              name = "b"
        "#);
    }

    /// A feature without coordinates, such as an empty multi-point, has no geometry field at all,
    /// as in the generated encoding.
    #[test]
    fn features_without_geometry_omit_the_field() -> MvtResult<()> {
        use geo_types::MultiPoint;

        let mut layer = MvtLayerBuilder::new("l")?;
        let mut feature = layer.feature_of(MvtGeomType::Point);
        feature.points([])?;
        layer = feature.end();
        layer = layer
            .feature(&MvtGeometry::MultiPoint(MultiPoint(vec![])))?
            .end();
        let bytes = layer.encode();

        let tile = Tile::decode_from_slice(&bytes)?;
        let features = &tile.layers[0].features;
        assert_eq!(features.len(), 2);
        assert!(features.iter().all(|feature| feature.geometry.is_empty()));
        assert_eq!(tile.encode_to_vec(), bytes);
        Ok(())
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

    #[test]
    fn packed_varints_match_the_generated_encoding() {
        let values = [
            0,
            1,
            0x7f,
            0x80,
            300,
            0x3fff,
            0x4000,
            0x1f_ffff,
            0x20_0000,
            u32::MAX,
        ];
        let mut expected = Vec::new();
        for &value in &values {
            buffa::types::encode_uint32(value, &mut expected);
        }
        let mut packed = vec![0xaa; 3];
        pack_varints(&values, &mut packed);
        assert_eq!(packed, expected);
        pack_varints(&[], &mut packed);
        assert_eq!(packed, b"");
    }

    /// The `Value` message the layer writes for `value`.
    fn encoded_value(value: &MvtValue) -> Vec<u8> {
        let mut scratch = [0; MAX_VARINT_LEN];
        let mut values = Interner::default();
        if let Some((kind, payload)) = value_payload(value.into(), &mut scratch) {
            values.index(kind, payload, None).unwrap();
        }
        values.encoded
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
            let body = proto.encode_to_vec();
            let mut expected = Vec::new();
            put_len_delimited_header(4, body.len() as u64, &mut expected);
            expected.extend_from_slice(&body);
            assert_eq!(encoded_value(&value), expected, "{value:?}");
        }
        assert_eq!(encoded_value(&MvtValue::Null), b"");
    }
}
