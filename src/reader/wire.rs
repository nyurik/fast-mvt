//! Reads a tile in one pass over its bytes, into a few vectors shared by the whole tile.
//!
//! Strings and the packed `tags` and `geometry` fields are borrowed from the tile bytes rather than
//! decoded into vectors of each feature's own. The packed fields are checked here, so decoding them
//! later can not fail. The errors are those of the generated view decoder, whose primitives this
//! uses, except that the same memory budget admits more of these smaller records.

use std::ops::Range;
use std::sync::OnceLock;

use buffa::encoding::{
    Tag, WireType, check_wire_type, count_varints, skip_field_depth, wire_type_mismatch,
};
use buffa::types::{
    borrow_bytes, borrow_str, decode_bool, decode_double, decode_float, decode_int32, decode_int64,
    decode_sint64, decode_uint32, decode_uint64,
};
use buffa::{DEFAULT_ELEMENT_MEMORY_LIMIT, DecodeError, Enumeration as _, RECURSION_LIMIT};
use usize_cast::IntoUsize as _;

use crate::MvtValueRef;
use crate::generated::vector_tile::tile::GeomType;

#[derive(Debug, Clone)]
pub(crate) struct TileData<'a> {
    /// The whole tile.
    pub(crate) data: &'a [u8],
    pub(crate) layers: Vec<LayerData<'a>>,
    /// The features of every layer, in order.
    features: Vec<FeatureData>,
    /// The keys of every layer, in order.
    keys: Vec<&'a str>,
    /// The values of every layer, in order.
    values: Vec<MvtValueRef<'a>>,
    /// Packed fields written in several pieces, joined here as they can not be borrowed whole.
    joined: Vec<u8>,
    /// The number of `tags` and `geometry` elements of all features.
    packed_count: u32,
    /// The `tags` and `geometry` of all features, decoded only when asked for as slices.
    decoded: OnceLock<Vec<u32>>,
}

#[derive(Debug, Clone)]
pub(crate) struct LayerData<'a> {
    pub(crate) name: &'a str,
    pub(crate) version: u32,
    pub(crate) extent: Option<u32>,
    features: Range<u32>,
    keys: Range<u32>,
    values: Range<u32>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct FeatureData {
    pub(crate) id: Option<u64>,
    pub(crate) geom_type: Option<GeomType>,
    tags: Packed,
    geometry: Packed,
    /// Where this feature's tags, followed by its geometry, start in [`TileData::decoded`].
    decoded_at: u32,
}

impl FeatureData {
    pub(crate) fn has_tags(&self) -> bool {
        self.tags.count > 0
    }
}

/// A packed `uint32` field, as the bytes of its checked varints.
#[derive(Debug, Clone, Copy, Default)]
struct Packed {
    start: u32,
    len: u32,
    /// The number of varints.
    count: u32,
    /// Whether the bytes are in [`TileData::joined`] rather than the tile.
    joined: bool,
}

impl<'a> TileData<'a> {
    pub(crate) fn decode(data: &'a [u8]) -> Result<Self, DecodeError> {
        to_u32(data.len())?;
        let mut tile = Self {
            data,
            layers: Vec::new(),
            features: Vec::new(),
            keys: Vec::new(),
            values: Vec::new(),
            joined: Vec::new(),
            packed_count: 0,
            decoded: OnceLock::new(),
        };
        let mut budget = Budget(DEFAULT_ELEMENT_MEMORY_LIMIT);
        let mut cur = data;
        while !cur.is_empty() {
            let tag = Tag::decode(&mut cur)?;
            if tag.field_number() == 3 {
                check_wire_type(tag, WireType::LengthDelimited)?;
                let layer = read_bytes(&mut cur)?;
                let offset = data.len() - cur.len() - layer.len();
                budget.charge(size_of::<LayerData<'_>>())?;
                tile.read_layer(layer, offset, &mut budget)?;
            } else {
                skip_field_depth(tag, &mut cur, RECURSION_LIMIT)?;
            }
        }
        Ok(tile)
    }

    /// Reads a layer, whose bytes `buf` start at `offset` in the tile.
    fn read_layer(
        &mut self,
        buf: &'a [u8],
        offset: usize,
        budget: &mut Budget,
    ) -> Result<(), DecodeError> {
        let (features, keys, values) = (self.features.len(), self.keys.len(), self.values.len());
        let mut name = "";
        let mut version = 0;
        let mut extent = None;
        let mut cur = buf;
        while !cur.is_empty() {
            let tag = Tag::decode(&mut cur)?;
            match tag.field_number() {
                15 => {
                    check_wire_type(tag, WireType::Varint)?;
                    version = decode_uint32(&mut cur)?;
                }
                1 => {
                    check_wire_type(tag, WireType::LengthDelimited)?;
                    name = borrow_str(&mut cur)?;
                }
                5 => {
                    check_wire_type(tag, WireType::Varint)?;
                    extent = Some(decode_uint32(&mut cur)?);
                }
                2 => {
                    check_wire_type(tag, WireType::LengthDelimited)?;
                    let feature = read_bytes(&mut cur)?;
                    let feature_offset = offset + buf.len() - cur.len() - feature.len();
                    budget.charge(size_of::<FeatureData>())?;
                    self.read_feature(feature, feature_offset)?;
                }
                3 => {
                    check_wire_type(tag, WireType::LengthDelimited)?;
                    let key = borrow_str(&mut cur)?;
                    budget.charge(size_of::<&str>())?;
                    self.keys.push(key);
                }
                4 => {
                    check_wire_type(tag, WireType::LengthDelimited)?;
                    let value = read_bytes(&mut cur)?;
                    budget.charge(size_of::<MvtValueRef<'_>>())?;
                    self.values.push(read_value(value)?);
                }
                _ => skip_field_depth(tag, &mut cur, RECURSION_LIMIT - 1)?,
            }
        }
        self.layers.push(LayerData {
            name,
            version,
            extent,
            features: range(features, self.features.len())?,
            keys: range(keys, self.keys.len())?,
            values: range(values, self.values.len())?,
        });
        Ok(())
    }

    /// Reads a feature, whose bytes `buf` start at `offset` in the tile.
    fn read_feature(&mut self, buf: &'a [u8], offset: usize) -> Result<(), DecodeError> {
        let mut feature = FeatureData {
            decoded_at: self.packed_count,
            ..FeatureData::default()
        };
        let mut tags = PackedReader::default();
        let mut geometry = PackedReader::default();
        let mut cur = buf;
        while !cur.is_empty() {
            let tag = Tag::decode(&mut cur)?;
            match tag.field_number() {
                1 => {
                    check_wire_type(tag, WireType::Varint)?;
                    feature.id = Some(decode_uint64(&mut cur)?);
                }
                3 => {
                    check_wire_type(tag, WireType::Varint)?;
                    // A value the enum does not know is dropped, as the generated decoder does.
                    if let Some(geom_type) = GeomType::from_i32(decode_int32(&mut cur)?) {
                        feature.geom_type = Some(geom_type);
                    }
                }
                2 => {
                    let at = offset + buf.len() - cur.len();
                    tags.read(tag, &mut cur, at)?;
                }
                4 => {
                    let at = offset + buf.len() - cur.len();
                    geometry.read(tag, &mut cur, at)?;
                }
                _ => skip_field_depth(tag, &mut cur, RECURSION_LIMIT - 2)?,
            }
        }
        feature.tags = tags.finish(&mut self.joined)?;
        feature.geometry = geometry.finish(&mut self.joined)?;
        self.packed_count += feature.tags.count + feature.geometry.count;
        self.features.push(feature);
        Ok(())
    }

    pub(crate) fn features(&self, layer: &LayerData<'_>) -> &[FeatureData] {
        &self.features[to_usize(&layer.features)]
    }

    pub(crate) fn keys(&self, layer: &LayerData<'_>) -> &[&'a str] {
        &self.keys[to_usize(&layer.keys)]
    }

    pub(crate) fn values(&self, layer: &LayerData<'_>) -> &[MvtValueRef<'a>] {
        &self.values[to_usize(&layer.values)]
    }

    pub(crate) fn tags(&self, feature: &FeatureData) -> PackedU32<'_> {
        self.packed(feature.tags)
    }

    pub(crate) fn geometry(&self, feature: &FeatureData) -> PackedU32<'_> {
        self.packed(feature.geometry)
    }

    pub(crate) fn tags_slice(&self, feature: &FeatureData) -> &[u32] {
        let start = feature.decoded_at.into_usize();
        &self.decoded()[start..start + feature.tags.count.into_usize()]
    }

    pub(crate) fn geometry_slice(&self, feature: &FeatureData) -> &[u32] {
        let start = (feature.decoded_at + feature.tags.count).into_usize();
        &self.decoded()[start..start + feature.geometry.count.into_usize()]
    }

    /// The `tags` and `geometry` of every feature, decoded the first time they are needed.
    fn decoded(&self) -> &[u32] {
        self.decoded.get_or_init(|| {
            let mut decoded = Vec::with_capacity(self.packed_count.into_usize());
            for feature in &self.features {
                decoded.extend(self.tags(feature));
                decoded.extend(self.geometry(feature));
            }
            decoded
        })
    }

    fn packed(&self, packed: Packed) -> PackedU32<'_> {
        let source = if packed.joined {
            &self.joined
        } else {
            self.data
        };
        let start = packed.start.into_usize();
        PackedU32 {
            bytes: &source[start..start + packed.len.into_usize()],
            remaining: packed.count.into_usize(),
        }
    }
}

/// The value a `Value` message holds, as the first of its fields set in the order they are
/// declared. A message with no known field set is [`MvtValueRef::Null`].
fn read_value(buf: &[u8]) -> Result<MvtValueRef<'_>, DecodeError> {
    let mut string = None;
    let mut float = None;
    let mut double = None;
    let mut int = None;
    let mut uint = None;
    let mut sint = None;
    let mut bool = None;
    let mut cur = buf;
    while !cur.is_empty() {
        let tag = Tag::decode(&mut cur)?;
        match tag.field_number() {
            1 => {
                check_wire_type(tag, WireType::LengthDelimited)?;
                string = Some(borrow_str(&mut cur)?);
            }
            2 => {
                check_wire_type(tag, WireType::Fixed32)?;
                float = Some(decode_float(&mut cur)?);
            }
            3 => {
                check_wire_type(tag, WireType::Fixed64)?;
                double = Some(decode_double(&mut cur)?);
            }
            4 => {
                check_wire_type(tag, WireType::Varint)?;
                int = Some(decode_int64(&mut cur)?);
            }
            5 => {
                check_wire_type(tag, WireType::Varint)?;
                uint = Some(decode_uint64(&mut cur)?);
            }
            6 => {
                check_wire_type(tag, WireType::Varint)?;
                sint = Some(decode_sint64(&mut cur)?);
            }
            7 => {
                check_wire_type(tag, WireType::Varint)?;
                bool = Some(decode_bool(&mut cur)?);
            }
            _ => skip_field_depth(tag, &mut cur, RECURSION_LIMIT - 2)?,
        }
    }
    Ok(if let Some(value) = string {
        MvtValueRef::String(value)
    } else if let Some(value) = float {
        MvtValueRef::Float(value)
    } else if let Some(value) = double {
        MvtValueRef::Double(value)
    } else if let Some(value) = int {
        MvtValueRef::Int(value)
    } else if let Some(value) = uint {
        MvtValueRef::UInt(value)
    } else if let Some(value) = sint {
        MvtValueRef::SInt(value)
    } else if let Some(value) = bool {
        MvtValueRef::Bool(value)
    } else {
        MvtValueRef::Null
    })
}

/// Gathers the records of one packed `uint32` field of a feature. A field is usually one packed
/// record, borrowed as is; anything else is joined into one run of varints.
#[derive(Default)]
struct PackedReader<'a> {
    /// The bytes of the field's only record so far, and where they start in the tile.
    bytes: &'a [u8],
    start: usize,
    joined: Option<Vec<u8>>,
    count: u32,
}

impl<'a> PackedReader<'a> {
    /// Reads a record from `cur`, which starts at `offset` in the tile.
    fn read(&mut self, tag: Tag, cur: &mut &'a [u8], offset: usize) -> Result<(), DecodeError> {
        let before = *cur;
        let bytes = match tag.wire_type() {
            WireType::LengthDelimited => {
                let payload = read_bytes(cur)?;
                self.count += check_varints(payload)?;
                payload
            }
            // A single element, not packed.
            WireType::Varint => {
                decode_uint32(cur)?;
                self.count += 1;
                &before[..before.len() - cur.len()]
            }
            _ => return Err(wire_type_mismatch(tag, WireType::LengthDelimited)),
        };
        if let Some(joined) = &mut self.joined {
            joined.extend_from_slice(bytes);
        } else if self.bytes.is_empty() {
            self.bytes = bytes;
            // The record's bytes end where `cur` now starts.
            self.start = offset + before.len() - cur.len() - bytes.len();
        } else if !bytes.is_empty() {
            self.joined = Some([self.bytes, bytes].concat());
        }
        Ok(())
    }

    fn finish(self, joined: &mut Vec<u8>) -> Result<Packed, DecodeError> {
        Ok(if let Some(bytes) = self.joined {
            let start = to_u32(joined.len())?;
            joined.extend_from_slice(&bytes);
            Packed {
                start,
                len: to_u32(bytes.len())?,
                count: self.count,
                joined: true,
            }
        } else {
            Packed {
                start: to_u32(self.start)?,
                len: to_u32(self.bytes.len())?,
                count: self.count,
                joined: false,
            }
        })
    }
}

/// Checks that `payload` is a run of varints as `uint32` decoding accepts them, and counts them.
fn check_varints(payload: &[u8]) -> Result<u32, DecodeError> {
    match payload.last() {
        None => return Ok(0),
        // An unterminated last varint: decoding each one finds the error the generated decoder does,
        // at the latest when no bytes are left.
        Some(&last) if last >= 0x80 => {
            let mut cur = payload;
            loop {
                decode_uint32(&mut cur)?;
            }
        }
        Some(_) => {}
    }
    // A varint is at most 10 bytes: 9 continuation bytes, then one holding only the 64th bit. Any
    // run of 9 continuation bytes covers one of every 9 bytes, so only those few are looked at.
    let mut idx = 8;
    while let Some(&byte) = payload.get(idx) {
        if byte < 0x80 {
            idx += 9;
            continue;
        }
        let start = payload[..idx]
            .iter()
            .rposition(|&byte| byte < 0x80)
            .map_or(0, |pos| pos + 1);
        // The last byte ends a varint, so there is an end.
        let end = idx
            + payload[idx..]
                .iter()
                .take_while(|&&byte| byte >= 0x80)
                .count();
        if end - start >= 10 || (end - start == 9 && payload[end] > 1) {
            return Err(DecodeError::VarintTooLong);
        }
        idx = end + 9;
    }
    to_u32(count_varints(payload))
}

/// Borrows a length-delimited field, with a fast path for the usual one-byte length.
fn read_bytes<'a>(cur: &mut &'a [u8]) -> Result<&'a [u8], DecodeError> {
    if let Some((&len, rest)) = cur.split_first() {
        let len = usize::from(len);
        if len < 0x80 && len <= rest.len() {
            let (bytes, rest) = rest.split_at(len);
            *cur = rest;
            return Ok(bytes);
        }
    }
    borrow_bytes(cur)
}

/// The varints of a checked packed field, decoded as `uint32` values.
#[derive(Debug, Clone)]
pub(crate) struct PackedU32<'a> {
    bytes: &'a [u8],
    remaining: usize,
}

impl Iterator for PackedU32<'_> {
    type Item = u32;

    fn next(&mut self) -> Option<u32> {
        let (&first, mut rest) = self.bytes.split_first()?;
        self.remaining -= 1;
        if first < 0x80 {
            self.bytes = rest;
            return Some(u32::from(first));
        }
        // A `uint32` keeps the low 32 bits of a longer varint.
        let mut value = u32::from(first & 0x7f);
        let mut shift = 7;
        while let Some((&byte, tail)) = rest.split_first() {
            rest = tail;
            if shift < 32 {
                value |= u32::from(byte & 0x7f) << shift;
            }
            if byte < 0x80 {
                break;
            }
            shift += 7;
        }
        self.bytes = rest;
        Some(value)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }
}

impl ExactSizeIterator for PackedU32<'_> {}

/// What is left of the memory the decoded tile may take, with the generated decoder's limit.
struct Budget(usize);

impl Budget {
    fn charge(&mut self, bytes: usize) -> Result<(), DecodeError> {
        self.0 = (self.0)
            .checked_sub(bytes)
            .ok_or(DecodeError::ElementMemoryLimitExceeded)?;
        Ok(())
    }
}

/// A size or position within the tile, as tiles are limited to what a `u32` can address.
fn to_u32(value: usize) -> Result<u32, DecodeError> {
    u32::try_from(value).map_err(|_| DecodeError::MessageTooLarge)
}

fn range(start: usize, end: usize) -> Result<Range<u32>, DecodeError> {
    Ok(to_u32(start)?..to_u32(end)?)
}

fn to_usize(range: &Range<u32>) -> Range<usize> {
    range.start.into_usize()..range.end.into_usize()
}

#[cfg(test)]
pub(crate) mod tests {
    use buffa::Message as _;
    use buffa::types::{encode_uint32, put_len_delimited_header, put_string_field};
    use geo_types::{Geometry, MultiPoint, point};

    use super::*;
    use crate::MvtReaderRef;
    use crate::generated::vector_tile::Tile;

    fn put_varint(field: u32, value: u32, buf: &mut Vec<u8>) {
        Tag::new(field, WireType::Varint).encode(buf);
        encode_uint32(value, buf);
    }

    fn put_bytes(field: u32, payload: &[u8], buf: &mut Vec<u8>) {
        put_len_delimited_header(field, payload.len() as u64, buf);
        buf.extend_from_slice(payload);
    }

    /// A tile of one layer, named `l` and of version 2, followed by `fields`.
    fn tile_of_layer(fields: &[u8]) -> Vec<u8> {
        let mut layer = Vec::new();
        put_string_field(1, "l", &mut layer);
        put_varint(15, 2, &mut layer);
        layer.extend_from_slice(fields);
        let mut tile = Vec::new();
        put_bytes(3, &layer, &mut tile);
        tile
    }

    /// A tile of one layer holding `count` empty length-delimited fields numbered `field`.
    pub(crate) fn layer_of_empty_fields(field: u32, count: usize) -> Vec<u8> {
        let mut fields = Vec::new();
        for _ in 0..count {
            put_bytes(field, &[], &mut fields);
        }
        tile_of_layer(&fields)
    }

    /// The number of varints in `payload`, decoding each one as the generated decoder does.
    fn decode_each(payload: &[u8]) -> Result<u32, DecodeError> {
        let mut cur = payload;
        let mut count = 0;
        while !cur.is_empty() {
            decode_uint32(&mut cur)?;
            count += 1;
        }
        Ok(count)
    }

    fn decode(bytes: &[u8]) -> Vec<u32> {
        let count = check_varints(bytes).unwrap();
        let values: Vec<_> = PackedU32 {
            bytes,
            remaining: count.into_usize(),
        }
        .collect();
        assert_eq!(values.len(), count.into_usize());
        values
    }

    #[test]
    fn packed_varints_decode_like_uint32() {
        assert_eq!(decode(&[]), Vec::<u32>::new());
        assert_eq!(decode(&[0, 1, 0x7f]), [0, 1, 0x7f]);
        assert_eq!(decode(&[0x80, 0x01, 0xac, 0x02]), [128, 300]);
        assert_eq!(decode(&[0xff, 0xff, 0xff, 0xff, 0x0f]), [u32::MAX]);
        // Longer varints keep their low 32 bits, as for a negative `int32` written as `uint32`.
        let minus_one = [0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x01];
        assert_eq!(decode(&minus_one), [u32::MAX]);
        assert_eq!(decode(&[0x80, 0x80, 0x80, 0x80, 0x80, 0x01, 5]), [0, 5]);
    }

    /// Runs of continuation bytes of every length up to too long, at every offset from the bytes
    /// `check_varints` samples, ended or not, check as decoding each varint does.
    #[test]
    fn packed_varint_checks_match_decoding_each() {
        for before in 0..10 {
            for run in 0..12 {
                for last in [None, Some(0x00), Some(0x01), Some(0x02), Some(0x7f)] {
                    for after in 0..3 {
                        let mut payload = vec![0x01; before];
                        payload.extend(std::iter::repeat_n(0xff, run));
                        payload.extend(last);
                        payload.extend(std::iter::repeat_n(0x01, after));
                        assert_eq!(
                            check_varints(&payload),
                            decode_each(&payload),
                            "{payload:x?}"
                        );
                    }
                }
            }
        }
    }

    /// Length-delimited fields of every length class, whole or cut short, are borrowed as buffa
    /// borrows them.
    #[test]
    fn bytes_are_read_like_borrow_bytes() {
        for len in [0, 1, 127, 128, 300] {
            let mut field = Vec::new();
            encode_uint32(len, &mut field);
            field.extend(std::iter::repeat_n(7, len.into_usize()));
            field.push(0);
            for end in [field.len(), field.len() - 2] {
                let (mut read, mut borrowed) = (&field[..end], &field[..end]);
                assert_eq!(
                    read_bytes(&mut read),
                    borrow_bytes(&mut borrowed),
                    "{len} {end}"
                );
                assert_eq!(read, borrowed);
            }
        }
    }

    /// Unknown fields are skipped at every level, and a known one of the wrong wire type fails.
    #[test]
    fn unknown_fields_are_skipped_and_wire_types_checked() {
        let mut value = Vec::new();
        put_varint(5, 5, &mut value);
        put_varint(9, 7, &mut value);
        let mut feature = Vec::new();
        put_bytes(2, &[0, 0], &mut feature);
        put_varint(9, 7, &mut feature);
        let mut fields = Vec::new();
        put_string_field(3, "k", &mut fields);
        put_varint(6, 7, &mut fields);
        put_bytes(4, &value, &mut fields);
        put_bytes(2, &feature, &mut fields);
        let mut tile = Vec::new();
        put_varint(1, 7, &mut tile);
        tile.extend(tile_of_layer(&fields));

        let decoded = TileData::decode(&tile).unwrap();
        let expected = Tile::decode_from_slice(&tile).unwrap();
        assert_eq!(decoded.keys, ["k"]);
        assert_eq!(decoded.values, [MvtValueRef::UInt(5)]);
        assert_eq!(
            decoded.tags_slice(&decoded.features[0]),
            expected.layers[0].features[0].tags
        );

        let mut feature = Vec::new();
        Tag::new(2, WireType::Fixed32).encode(&mut feature);
        feature.extend([0; 4]);
        let mut fields = Vec::new();
        put_bytes(2, &feature, &mut fields);
        assert!(matches!(
            TileData::decode(&tile_of_layer(&fields)),
            Err(DecodeError::WireTypeMismatch {
                field_number: 2,
                ..
            })
        ));
    }

    #[test]
    fn memory_budget_limits_the_number_of_features() {
        let most =
            (DEFAULT_ELEMENT_MEMORY_LIMIT - size_of::<LayerData<'_>>()) / size_of::<FeatureData>();
        let bytes = layer_of_empty_fields(2, most);
        assert_eq!(TileData::decode(&bytes).unwrap().features.len(), most);
        assert!(matches!(
            TileData::decode(&layer_of_empty_fields(2, most + 1)),
            Err(DecodeError::ElementMemoryLimitExceeded)
        ));
    }

    /// Fields given in several packed and unpacked records read as their concatenation, as the
    /// generated decoder reads them.
    #[test]
    fn split_packed_fields_are_joined() {
        let packed = |field, values: &[u32], buf: &mut Vec<u8>| {
            let mut payload = Vec::new();
            for &value in values {
                encode_uint32(value, &mut payload);
            }
            put_bytes(field, &payload, buf);
        };
        let mut fields = Vec::new();
        put_string_field(3, "a", &mut fields);
        put_string_field(3, "b", &mut fields);
        let mut string = Vec::new();
        put_string_field(1, "x", &mut string);
        put_bytes(4, &string, &mut fields);
        let mut uint = Vec::new();
        put_varint(5, 5, &mut uint);
        put_bytes(4, &uint, &mut fields);
        // Two features, so that joined bytes of one do not run into those of the other.
        for offset in [0, 1] {
            let mut feature = Vec::new();
            packed(2, &[0, offset], &mut feature);
            packed(4, &[17, 4], &mut feature);
            put_varint(2, 1, &mut feature);
            put_varint(4, 6, &mut feature);
            packed(2, &[], &mut feature);
            packed(4, &[], &mut feature);
            packed(2, &[1 - offset], &mut feature);
            packed(4, &[2, 2], &mut feature);
            put_varint(3, 1, &mut feature);
            put_bytes(2, &feature, &mut fields);
        }
        let tile = tile_of_layer(&fields);

        let expected = Tile::decode_from_slice(&tile).unwrap();
        let reader = MvtReaderRef::new(&tile).unwrap();
        let layer = reader.layers().next().unwrap();
        for (feature, proto) in layer.features().zip(&expected.layers[0].features) {
            assert_eq!(feature.tags(), proto.tags);
            assert_eq!(feature.geometry_commands(), proto.geometry);
            assert_eq!(
                feature.geometry().unwrap(),
                Geometry::MultiPoint(MultiPoint(vec![
                    point! { x: 2, y: 3 },
                    point! { x: 3, y: 4 }
                ]))
            );
        }
        let properties: Vec<_> = layer
            .features()
            .map(|feature| feature.properties_vec().unwrap())
            .collect();
        assert_eq!(
            properties,
            [
                [("a", MvtValueRef::String("x")), ("b", MvtValueRef::UInt(5))],
                [("a", MvtValueRef::UInt(5)), ("b", MvtValueRef::String("x"))],
            ]
        );
    }
}
