use std::borrow::Cow;
use std::num::NonZeroU32;

use geo_types::{
    Coord, Geometry, LineString, MultiLineString, MultiPoint, MultiPolygon, Point, Polygon,
};

pub type MvtExtent = NonZeroU32;
pub type MvtCoord = Coord<i32>;
pub type MvtPoint = Point<i32>;
pub type MvtLineString = LineString<i32>;
pub type MvtPolygon = Polygon<i32>;
pub type MvtMultiPoint = MultiPoint<i32>;
pub type MvtMultiLineString = MultiLineString<i32>;
pub type MvtMultiPolygon = MultiPolygon<i32>;
pub type MvtGeometry = Geometry<i32>;

pub const DEFAULT_EXTENT: MvtExtent = MvtExtent::new(4096).unwrap();

#[derive(Debug, Clone, Default, PartialEq)]
pub struct MvtTile {
    pub layers: Vec<MvtLayer>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MvtLayer {
    pub name: String,
    pub extent: NonZeroU32,
    pub features: Vec<MvtFeature>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MvtFeature {
    pub id: Option<u64>,
    pub geometry: MvtGeometry,
    pub properties: Vec<(String, MvtValue)>,
}

impl MvtTile {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_layer(&mut self, layer: MvtLayer) {
        self.layers.push(layer);
    }

    #[cfg(feature = "writer")]
    pub fn encode(self) -> crate::MvtResult<Vec<u8>> {
        crate::writer::encode_tile(self)
    }

    #[cfg(feature = "writer")]
    pub fn encode_ref(&self) -> crate::MvtResult<Vec<u8>> {
        crate::writer::encode_tile_ref(self)
    }
}

impl MvtLayer {
    #[must_use]
    pub fn new(name: impl Into<String>, extent: MvtExtent) -> Self {
        Self {
            name: name.into(),
            extent,
            features: Vec::new(),
        }
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub fn num_features(&self) -> usize {
        self.features.len()
    }

    pub fn add_feature(&mut self, feature: MvtFeature) {
        self.features.push(feature);
    }
}

impl MvtFeature {
    #[must_use]
    pub fn new(geometry: MvtGeometry) -> Self {
        Self {
            id: None,
            geometry,
            properties: Vec::new(),
        }
    }

    pub fn set_id(&mut self, id: u64) {
        self.id = Some(id);
    }

    #[must_use]
    pub fn num_tags(&self) -> usize {
        self.properties.len()
    }

    pub fn add_tag(&mut self, key: impl Into<String>, value: MvtValue) {
        self.properties.push((key.into(), value));
    }

    pub fn add_tag_string(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.add_tag(key, MvtValue::String(value.into()));
    }

    pub fn add_tag_float(&mut self, key: impl Into<String>, value: f32) {
        self.add_tag(key, MvtValue::Float(value));
    }

    pub fn add_tag_double(&mut self, key: impl Into<String>, value: f64) {
        self.add_tag(key, MvtValue::Double(value));
    }

    pub fn add_tag_int(&mut self, key: impl Into<String>, value: i64) {
        self.add_tag(key, MvtValue::Int(value));
    }

    pub fn add_tag_uint(&mut self, key: impl Into<String>, value: u64) {
        self.add_tag(key, MvtValue::UInt(value));
    }

    pub fn add_tag_sint(&mut self, key: impl Into<String>, value: i64) {
        self.add_tag(key, MvtValue::SInt(value));
    }

    pub fn add_tag_bool(&mut self, key: impl Into<String>, value: bool) {
        self.add_tag(key, MvtValue::Bool(value));
    }
}

#[derive(Debug, Clone)]
pub enum MvtValue {
    String(String),
    Float(f32),
    Double(f64),
    Int(i64),
    UInt(u64),
    SInt(i64),
    Bool(bool),
    Null,
}

impl From<String> for MvtValue {
    fn from(value: String) -> Self {
        Self::String(value)
    }
}

impl From<&str> for MvtValue {
    fn from(value: &str) -> Self {
        Self::String(value.to_string())
    }
}

impl From<Cow<'_, str>> for MvtValue {
    fn from(value: Cow<'_, str>) -> Self {
        Self::String(value.into_owned())
    }
}

impl From<f32> for MvtValue {
    fn from(value: f32) -> Self {
        Self::Float(value)
    }
}

impl From<f64> for MvtValue {
    fn from(value: f64) -> Self {
        Self::Double(value)
    }
}

impl From<i64> for MvtValue {
    fn from(value: i64) -> Self {
        Self::Int(value)
    }
}

impl From<i32> for MvtValue {
    fn from(value: i32) -> Self {
        Self::Int(i64::from(value))
    }
}

impl From<i16> for MvtValue {
    fn from(value: i16) -> Self {
        Self::Int(i64::from(value))
    }
}

impl From<i8> for MvtValue {
    fn from(value: i8) -> Self {
        Self::Int(i64::from(value))
    }
}

impl From<u64> for MvtValue {
    fn from(value: u64) -> Self {
        Self::UInt(value)
    }
}

impl From<u32> for MvtValue {
    fn from(value: u32) -> Self {
        Self::UInt(u64::from(value))
    }
}

impl From<u16> for MvtValue {
    fn from(value: u16) -> Self {
        Self::UInt(u64::from(value))
    }
}

impl From<u8> for MvtValue {
    fn from(value: u8) -> Self {
        Self::UInt(u64::from(value))
    }
}

impl From<bool> for MvtValue {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}

#[cfg(feature = "json")]
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum MvtJsonValueError {
    #[error("non-finite float cannot be represented as JSON")]
    NonFiniteFloat,
    #[error("invalid JSON number")]
    InvalidJsonNumber,
    #[error("JSON array cannot be represented as an MVT value")]
    UnsupportedJsonArray,
    #[error("JSON object cannot be represented as an MVT value")]
    UnsupportedJsonObject,
}

#[cfg(feature = "json")]
impl TryFrom<MvtValue> for serde_json::Value {
    type Error = MvtJsonValueError;

    fn try_from(value: MvtValue) -> Result<Self, Self::Error> {
        match value {
            MvtValue::String(value) => Ok(Self::String(value)),
            MvtValue::Float(value) => json_number(f64::from(value)),
            MvtValue::Double(value) => json_number(value),
            MvtValue::Int(value) | MvtValue::SInt(value) => Ok(value.into()),
            MvtValue::UInt(value) => Ok(value.into()),
            MvtValue::Bool(value) => Ok(Self::Bool(value)),
            MvtValue::Null => Ok(Self::Null),
        }
    }
}

#[cfg(feature = "json")]
impl TryFrom<serde_json::Value> for MvtValue {
    type Error = MvtJsonValueError;

    fn try_from(value: serde_json::Value) -> Result<Self, Self::Error> {
        use serde_json::Value;

        match value {
            Value::Null => Ok(Self::Null),
            Value::Bool(value) => Ok(Self::Bool(value)),
            Value::Number(value) => {
                if let Some(value) = value.as_i64() {
                    Ok(Self::Int(value))
                } else if let Some(value) = value.as_u64() {
                    Ok(Self::UInt(value))
                } else if let Some(value) = value.as_f64() {
                    Ok(Self::Double(value))
                } else {
                    Err(MvtJsonValueError::InvalidJsonNumber)
                }
            }
            Value::String(value) => Ok(Self::String(value)),
            Value::Array(_) => Err(MvtJsonValueError::UnsupportedJsonArray),
            Value::Object(_) => Err(MvtJsonValueError::UnsupportedJsonObject),
        }
    }
}

#[cfg(feature = "json")]
fn json_number(value: f64) -> Result<serde_json::Value, MvtJsonValueError> {
    serde_json::Number::from_f64(value)
        .map(serde_json::Value::Number)
        .ok_or(MvtJsonValueError::NonFiniteFloat)
}

impl PartialEq for MvtValue {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::String(a), Self::String(b)) => a == b,
            (Self::Float(a), Self::Float(b)) => a.to_bits() == b.to_bits(),
            (Self::Double(a), Self::Double(b)) => a.to_bits() == b.to_bits(),
            (Self::Int(a), Self::Int(b)) | (Self::SInt(a), Self::SInt(b)) => a == b,
            (Self::UInt(a), Self::UInt(b)) => a == b,
            (Self::Bool(a), Self::Bool(b)) => a == b,
            (Self::Null, Self::Null) => true,
            _ => false,
        }
    }
}

impl Eq for MvtValue {}

impl std::hash::Hash for MvtValue {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        std::mem::discriminant(self).hash(state);
        match self {
            Self::String(v) => v.hash(state),
            Self::Float(v) => v.to_bits().hash(state),
            Self::Double(v) => v.to_bits().hash(state),
            Self::Int(v) | Self::SInt(v) => v.hash(state),
            Self::UInt(v) => v.hash(state),
            Self::Bool(v) => v.hash(state),
            Self::Null => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;
    use std::hash::{Hash, Hasher};

    use super::*;

    #[test]
    fn owned_tile_layer_and_feature_helpers_mutate_expected_fields() {
        let mut feature = MvtFeature::new(MvtGeometry::Point((1, 2).into()));
        assert_eq!(feature.id, None);
        assert_eq!(feature.num_tags(), 0);

        feature.set_id(7);
        feature.add_tag("raw", MvtValue::Null);
        feature.add_tag_string("string", "value");
        feature.add_tag_float("float", 1.25);
        feature.add_tag_double("double", 2.5);
        feature.add_tag_int("int", -3);
        feature.add_tag_uint("uint", 4);
        feature.add_tag_sint("sint", -5);
        feature.add_tag_bool("bool", true);
        assert_eq!(feature.id, Some(7));
        assert_eq!(feature.num_tags(), 8);

        let mut layer = MvtLayer::new("places", DEFAULT_EXTENT);
        assert_eq!(layer.name(), "places");
        assert_eq!(layer.num_features(), 0);
        layer.add_feature(feature);
        assert_eq!(layer.num_features(), 1);

        let mut tile = MvtTile::new();
        assert!(tile.layers.is_empty());
        tile.add_layer(layer);
        assert_eq!(tile.layers.len(), 1);
    }

    #[test]
    fn mvt_value_from_impls_preserve_variant_intent() {
        assert_eq!(
            MvtValue::from(String::from("owned")),
            MvtValue::String("owned".into())
        );
        assert_eq!(
            MvtValue::from("borrowed"),
            MvtValue::String("borrowed".into())
        );
        assert_eq!(
            MvtValue::from(Cow::Borrowed("cow")),
            MvtValue::String("cow".into())
        );
        assert_eq!(MvtValue::from(1.25_f32), MvtValue::Float(1.25));
        assert_eq!(MvtValue::from(2.5_f64), MvtValue::Double(2.5));
        assert_eq!(MvtValue::from(-3_i64), MvtValue::Int(-3));
        assert_eq!(MvtValue::from(-4_i32), MvtValue::Int(-4));
        assert_eq!(MvtValue::from(-5_i16), MvtValue::Int(-5));
        assert_eq!(MvtValue::from(-6_i8), MvtValue::Int(-6));
        assert_eq!(MvtValue::from(7_u64), MvtValue::UInt(7));
        assert_eq!(MvtValue::from(8_u32), MvtValue::UInt(8));
        assert_eq!(MvtValue::from(9_u16), MvtValue::UInt(9));
        assert_eq!(MvtValue::from(10_u8), MvtValue::UInt(10));
        assert_eq!(MvtValue::from(true), MvtValue::Bool(true));
    }

    #[test]
    fn mvt_value_equality_and_hash_include_variant_and_float_bits() {
        #[derive(Default)]
        struct RecordingHasher(Vec<u8>);

        impl Hasher for RecordingHasher {
            fn finish(&self) -> u64 {
                0
            }

            fn write(&mut self, bytes: &[u8]) {
                self.0.extend_from_slice(bytes);
            }
        }

        fn hash_bytes(value: &MvtValue) -> Vec<u8> {
            let mut hasher = RecordingHasher::default();
            value.hash(&mut hasher);
            hasher.0
        }

        assert_eq!(MvtValue::Float(f32::NAN), MvtValue::Float(f32::NAN));
        assert_ne!(MvtValue::Int(1), MvtValue::SInt(1));
        assert_ne!(MvtValue::Int(1), MvtValue::UInt(1));
        assert_eq!(
            hash_bytes(&MvtValue::Double(f64::NAN)),
            hash_bytes(&MvtValue::Double(f64::NAN))
        );
        assert_ne!(
            hash_bytes(&MvtValue::Int(1)),
            hash_bytes(&MvtValue::SInt(1))
        );
        assert_eq!(hash_bytes(&MvtValue::Null), hash_bytes(&MvtValue::Null));
    }
}

#[cfg(all(test, feature = "json"))]
mod tests_json {
    use serde_json::json;

    use super::*;
    use crate::generated::vector_tile::Tile;

    #[test]
    fn mvt_values_convert_to_json_values() {
        assert_eq!(
            serde_json::Value::try_from(MvtValue::String("name".into())),
            Ok(json!("name"))
        );
        assert_eq!(
            serde_json::Value::try_from(MvtValue::Int(-3)),
            Ok(json!(-3))
        );
        assert_eq!(serde_json::Value::try_from(MvtValue::UInt(4)), Ok(json!(4)));
        assert_eq!(
            serde_json::Value::try_from(MvtValue::Double(1.5)),
            Ok(json!(1.5))
        );
        assert_eq!(
            serde_json::Value::try_from(MvtValue::Float(2.5)),
            Ok(json!(2.5))
        );
        assert_eq!(
            serde_json::Value::try_from(MvtValue::Bool(true)),
            Ok(json!(true))
        );
        assert_eq!(
            serde_json::Value::try_from(MvtValue::Null),
            Ok(serde_json::Value::Null)
        );
        assert_eq!(
            serde_json::Value::try_from(MvtValue::Double(f64::NAN)),
            Err(MvtJsonValueError::NonFiniteFloat)
        );
    }

    #[test]
    fn json_values_convert_to_mvt_values() {
        assert_eq!(
            MvtValue::try_from(json!("name")),
            Ok(MvtValue::String("name".into()))
        );
        assert_eq!(MvtValue::try_from(json!(-3)), Ok(MvtValue::Int(-3)));
        assert_eq!(
            MvtValue::try_from(json!(u64::MAX)),
            Ok(MvtValue::UInt(u64::MAX))
        );
        assert_eq!(MvtValue::try_from(json!(1.5)), Ok(MvtValue::Double(1.5)));
        assert_eq!(MvtValue::try_from(json!(true)), Ok(MvtValue::Bool(true)));
        assert_eq!(
            MvtValue::try_from(serde_json::Value::Null),
            Ok(MvtValue::Null)
        );
        assert_eq!(
            MvtValue::try_from(json!([])),
            Err(MvtJsonValueError::UnsupportedJsonArray)
        );
        assert_eq!(
            MvtValue::try_from(json!({})),
            Err(MvtJsonValueError::UnsupportedJsonObject)
        );
    }

    #[test]
    fn tile_deserializes_from_object_but_not_layers_array() {
        let object: Tile = serde_json::from_str(
            r#"{
                "layers": [{
                    "version": 2,
                    "name": "places",
                    "extent": 4096
                }]
            }"#,
        )
        .unwrap();

        let array = serde_json::from_str::<Tile>(
            r#"[{
                "version": 2,
                "name": "places",
                "extent": 4096
            }]"#,
        );

        assert!(array.is_err());
        assert_eq!(object.layers[0].name, "places");
    }
}
