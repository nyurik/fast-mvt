use usize_cast::IntoUsize;

use super::wire::PackedU32;
use crate::{MvtError, MvtResult, MvtValueRef};

/// A feature's properties: each tag's key and value, borrowed for `'t` from the layer they are in,
/// and for `'a` from the tile.
#[derive(Debug, Clone)]
pub struct MvtPropertyIter<'t, 'a> {
    keys: &'t [&'a str],
    values: &'t [MvtValueRef<'a>],
    tags: PackedU32<'t>,
}

impl<'t, 'a> MvtPropertyIter<'t, 'a> {
    pub(crate) fn new(
        keys: &'t [&'a str],
        values: &'t [MvtValueRef<'a>],
        tags: PackedU32<'t>,
    ) -> Self {
        Self { keys, values, tags }
    }
}

impl<'a> Iterator for MvtPropertyIter<'_, 'a> {
    type Item = MvtResult<(&'a str, MvtValueRef<'a>)>;

    fn next(&mut self) -> Option<Self::Item> {
        let key_idx = self.tags.next()?;
        let Some(value_idx) = self.tags.next() else {
            return Some(Err(MvtError::InvalidTagsLength(1)));
        };
        let Some(&key) = self.keys.get(key_idx.into_usize()) else {
            return Some(Err(MvtError::InvalidKeyIndex(key_idx)));
        };
        let Some(&value) = self.values.get(value_idx.into_usize()) else {
            return Some(Err(MvtError::InvalidValueIndex(value_idx)));
        };
        Some(Ok((key, value)))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let len = self.tags.len().div_ceil(2);
        (len, Some(len))
    }
}

/// Every tag pair, including a malformed trailing one, yields exactly one item.
impl ExactSizeIterator for MvtPropertyIter<'_, '_> {}

#[cfg(test)]
mod tests {
    use crate::MvtReaderRef;
    use crate::generated::vector_tile::tile as proto_tile;
    use crate::reader::tests::{encode_feature, first_feature};

    /// A tile of one feature with `tags`, in a layer of one key and one value.
    fn tile_with_tags(tags: Vec<u32>) -> Vec<u8> {
        let layer = proto_tile::Layer {
            version: 2,
            name: "tags".into(),
            keys: vec!["k".into()],
            values: vec![proto_tile::Value::default()],
            ..Default::default()
        };
        let feature = proto_tile::Feature {
            tags,
            ..Default::default()
        };
        encode_feature(layer, feature)
    }

    #[test]
    fn property_iterator_len_counts_a_malformed_trailing_tag() {
        let bytes = tile_with_tags(vec![0, 0, 0, 0, 0]);
        let reader = MvtReaderRef::new(&bytes).unwrap();
        let properties = first_feature(&reader).properties();
        assert_eq!(properties.len(), 3);
        assert_eq!(properties.count(), 3);
    }

    #[test]
    fn property_iterator_reports_malformed_tags() {
        for (tags, expected) in [
            (vec![0], "invalid feature tags length: 1"),
            (vec![1, 0], "invalid key index 1"),
            (vec![0, 1], "invalid value index 1"),
        ] {
            let bytes = tile_with_tags(tags);
            let reader = MvtReaderRef::new(&bytes).unwrap();
            let err = first_feature(&reader)
                .properties()
                .next()
                .unwrap()
                .unwrap_err();
            assert_eq!(err.to_string(), expected);
        }
    }
}
