use usize_cast::IntoUsize;

use crate::generated::vector_tile::tile as proto_tile;
use crate::{MvtError, MvtResult, MvtValueRef};

#[derive(Debug, Clone)]
pub struct MvtPropertyIter<'a> {
    keys: &'a [&'a str],
    values: &'a [proto_tile::ValueView<'a>],
    tags: std::slice::Chunks<'a, u32>,
}

impl<'a> MvtPropertyIter<'a> {
    pub(crate) fn new(
        keys: &'a [&'a str],
        values: &'a [proto_tile::ValueView<'a>],
        tags: std::slice::Chunks<'a, u32>,
    ) -> Self {
        Self { keys, values, tags }
    }
}

impl<'a> Iterator for MvtPropertyIter<'a> {
    type Item = MvtResult<(&'a str, MvtValueRef<'a>)>;

    fn next(&mut self) -> Option<Self::Item> {
        let pair = self.tags.next()?;
        let [key_idx, value_idx] = pair else {
            return Some(Err(MvtError::InvalidTagsLength(pair.len())));
        };
        let key = match self.keys.get((*key_idx).into_usize()) {
            Some(key) => *key,
            None => return Some(Err(MvtError::InvalidKeyIndex(*key_idx))),
        };
        let value = match self.values.get((*value_idx).into_usize()) {
            Some(value) => value_ref(value),
            None => return Some(Err(MvtError::InvalidValueIndex(*value_idx))),
        };
        Some(Ok((key, value)))
    }
}

pub(crate) fn value_ref<'a>(value: &'a proto_tile::ValueView<'a>) -> MvtValueRef<'a> {
    if let Some(value) = value.string_value {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MvtReaderRef;
    use crate::reader::tests::{encode_feature, first_feature};

    #[test]
    fn property_iterator_reports_malformed_tags() {
        let layer = proto_tile::Layer {
            version: 2,
            name: "tags".into(),
            keys: vec!["k".into()],
            values: vec![proto_tile::Value::default()],
            ..Default::default()
        };

        for (tags, expected) in [
            (vec![0], "invalid feature tags length: 1"),
            (vec![1, 0], "invalid key index 1"),
            (vec![0, 1], "invalid value index 1"),
        ] {
            let feature = proto_tile::Feature {
                tags,
                ..Default::default()
            };
            let bytes = encode_feature(layer.clone(), feature);
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
