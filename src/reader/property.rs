use std::fmt;

use usize_cast::IntoUsize;

use crate::generated::vector_tile::tile as proto_tile;
use crate::{MvtError, MvtResult, MvtValue};

#[derive(Debug, Clone)]
pub struct MvtPropertyIter<'a> {
    keys: &'a [&'a str],
    values: &'a [proto_tile::ValueView<'a>],
    tags: std::slice::Chunks<'a, u32>,
}

impl<'a> MvtPropertyIter<'a> {
    pub(super) fn new(
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

#[derive(Copy, Clone, PartialEq)]
pub enum MvtValueRef<'a> {
    String(&'a str),
    Float(f32),
    Double(f64),
    Int(i64),
    UInt(u64),
    SInt(i64),
    Bool(bool),
    Null,
}

impl MvtValueRef<'_> {
    #[must_use]
    pub fn into_owned(self) -> MvtValue {
        match self {
            Self::String(value) => MvtValue::String(value.to_string()),
            Self::Float(value) => MvtValue::Float(value),
            Self::Double(value) => MvtValue::Double(value),
            Self::Int(value) => MvtValue::Int(value),
            Self::UInt(value) => MvtValue::UInt(value),
            Self::SInt(value) => MvtValue::SInt(value),
            Self::Bool(value) => MvtValue::Bool(value),
            Self::Null => MvtValue::Null,
        }
    }

    /// The MVT type annotation used by the dump, or `None` for strings (whose
    /// quoting already makes the type obvious).
    pub(super) fn type_name(self) -> Option<&'static str> {
        match self {
            Self::String(_) => None,
            Self::Float(_) => Some("float"),
            Self::Double(_) => Some("double"),
            Self::Int(_) => Some("int"),
            Self::UInt(_) => Some("uint"),
            Self::SInt(_) => Some("sint"),
            Self::Bool(_) => Some("bool"),
            Self::Null => Some("null"),
        }
    }
}

impl fmt::Debug for MvtValueRef<'_> {
    /// Renders the bare textual value (strings quoted), without the variant
    /// name — the type is surfaced separately by [`MvtValueRef::type_name`].
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            MvtValueRef::String(value) => write!(f, "{value:?}"),
            MvtValueRef::Float(value) => write!(f, "{value}"),
            MvtValueRef::Double(value) => write!(f, "{value}"),
            MvtValueRef::Int(value) | MvtValueRef::SInt(value) => write!(f, "{value}"),
            MvtValueRef::UInt(value) => write!(f, "{value}"),
            MvtValueRef::Bool(value) => write!(f, "{value}"),
            MvtValueRef::Null => f.write_str("null"),
        }
    }
}

pub(super) fn value_ref<'a>(value: &'a proto_tile::ValueView<'a>) -> MvtValueRef<'a> {
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
    use crate::reader::tests::reader_from_layer;

    #[test]
    fn value_ref_debug_renders_bare_values() {
        assert_eq!(format!("{:?}", MvtValueRef::String("x")), "\"x\"");
        assert_eq!(format!("{:?}", MvtValueRef::Float(1.25)), "1.25");
        assert_eq!(format!("{:?}", MvtValueRef::Double(2.5)), "2.5");
        assert_eq!(format!("{:?}", MvtValueRef::Int(-3)), "-3");
        assert_eq!(format!("{:?}", MvtValueRef::UInt(4)), "4");
        assert_eq!(format!("{:?}", MvtValueRef::SInt(-5)), "-5");
        assert_eq!(format!("{:?}", MvtValueRef::Bool(true)), "true");
        assert_eq!(format!("{:?}", MvtValueRef::Null), "null");
    }

    #[test]
    fn value_ref_type_name_covers_every_variant() {
        assert_eq!(MvtValueRef::String("x").type_name(), None);
        assert_eq!(MvtValueRef::Float(1.25).type_name(), Some("float"));
        assert_eq!(MvtValueRef::Double(2.5).type_name(), Some("double"));
        assert_eq!(MvtValueRef::Int(-3).type_name(), Some("int"));
        assert_eq!(MvtValueRef::UInt(4).type_name(), Some("uint"));
        assert_eq!(MvtValueRef::SInt(-5).type_name(), Some("sint"));
        assert_eq!(MvtValueRef::Bool(true).type_name(), Some("bool"));
        assert_eq!(MvtValueRef::Null.type_name(), Some("null"));
    }

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
            let mut layer = layer.clone();
            layer.features = vec![feature];
            let reader = reader_from_layer(layer);
            let feature = reader.layers().next().unwrap().features().next().unwrap();
            let err = feature.properties().next().unwrap().unwrap_err();
            assert_eq!(err.to_string(), expected);
        }
    }
}
