use std::fmt;

use buffa::MessageView as _;

use super::MvtLayerRef;
use crate::generated::vector_tile::{Tile, TileView};
use crate::types::DEFAULT_EXTENT;
use crate::{MvtError, MvtResult, MvtTile};

impl Tile {
    #[must_use]
    pub fn from_reader(reader: &MvtReaderRef<'_>) -> Self {
        let mut tile = reader.to_proto();
        for layer in &mut tile.layers {
            layer.extent.get_or_insert(DEFAULT_EXTENT.get());
        }
        tile
    }
}

#[derive(Clone)]
pub struct MvtReaderRef<'a>(TileView<'a>);

impl<'a> MvtReaderRef<'a> {
    pub fn new(data: &'a [u8]) -> MvtResult<Self> {
        let tile = TileView::decode_view(data)?;
        for layer in &tile.layers {
            if layer.name.is_empty() {
                return Err(MvtError::MissingLayerName);
            }
            if layer.version < 1 || layer.version > 3 {
                return Err(MvtError::UnsupportedVersion {
                    layer: layer.name.to_string(),
                    version: layer.version,
                });
            }
        }
        Ok(Self(tile))
    }

    #[must_use]
    pub fn layers(&self) -> impl ExactSizeIterator<Item = MvtLayerRef<'_>> {
        self.0.layers.iter().map(MvtLayerRef::new)
    }

    #[must_use]
    pub fn layer_count(&self) -> usize {
        self.0.layers.len()
    }

    pub fn to_tile(&self) -> MvtResult<MvtTile> {
        let mut layers = Vec::with_capacity(self.0.layers.len());
        for layer in self.layers() {
            layers.push(layer.to_layer()?);
        }
        Ok(MvtTile { layers })
    }

    #[must_use]
    pub fn to_proto(&self) -> Tile {
        self.0.to_owned_message()
    }
}

impl fmt::Debug for MvtReaderRef<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, layer) in self.layers().enumerate() {
            if index > 0 {
                writeln!(
                    f,
                    "============================================================="
                )?;
            }
            writeln!(f, "layer: {index}")?;
            // Each layer's `Debug` block is already newline-terminated.
            write!(f, "{layer:?}")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use geo_types::Geometry;

    use super::*;
    use crate::MvtValue;
    use crate::generated::vector_tile::tile as proto_tile;
    use crate::reader::MvtValueRef;
    use crate::reader::tests::{encode_layer, first_feature};

    #[test]
    fn borrowed_api_reads_accessors_properties_and_repeated_points() {
        let layer = proto_tile::Layer {
            version: 3,
            name: "places".to_string(),
            keys: vec![
                "string".into(),
                "float".into(),
                "double".into(),
                "int".into(),
                "uint".into(),
                "sint".into(),
                "bool".into(),
                "null".into(),
            ],
            values: vec![
                proto_tile::Value {
                    string_value: Some("name".into()),
                    ..Default::default()
                },
                proto_tile::Value {
                    float_value: Some(1.25),
                    ..Default::default()
                },
                proto_tile::Value {
                    double_value: Some(2.5),
                    ..Default::default()
                },
                proto_tile::Value {
                    int_value: Some(-3),
                    ..Default::default()
                },
                proto_tile::Value {
                    uint_value: Some(4),
                    ..Default::default()
                },
                proto_tile::Value {
                    sint_value: Some(-5),
                    ..Default::default()
                },
                proto_tile::Value {
                    bool_value: Some(true),
                    ..Default::default()
                },
                proto_tile::Value::default(),
            ],
            features: vec![proto_tile::Feature {
                id: Some(7),
                tags: (0_u32..8).flat_map(|idx| [idx, idx]).collect(),
                r#type: Some(proto_tile::GeomType::Point),
                geometry: vec![9, 2, 4, 9, 6, 8],
            }],
            ..Default::default()
        };
        let bytes = encode_layer(layer);
        let reader = MvtReaderRef::new(&bytes).unwrap();
        let layer = reader.layers().next().unwrap();

        assert_eq!(reader.layer_count(), 1);
        assert_eq!(layer.name(), "places");
        assert_eq!(layer.version(), 3);
        assert_eq!(layer.extent(), DEFAULT_EXTENT.get());
        assert_eq!(layer.feature_count(), 1);
        assert!(!layer.is_empty());
        assert_eq!(
            layer.keys(),
            [
                "string", "float", "double", "int", "uint", "sint", "bool", "null"
            ]
        );
        assert_eq!(layer.values().len(), 8);

        let feature = layer.features().next().unwrap();
        assert_eq!(feature.id(), Some(7));
        assert!(feature.has_properties());
        assert_eq!(
            feature.tags(),
            [0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7]
        );
        assert_eq!(feature.geometry_commands(), [9, 2, 4, 9, 6, 8]);
        assert_eq!(feature.geom_type(), Some(proto_tile::GeomType::Point));
        assert_eq!(feature.geom_type_value(), Some(1));

        let properties = feature.properties_vec().unwrap();
        assert_eq!(properties[0].1, MvtValueRef::String("name"));
        assert_eq!(properties[1].1, MvtValueRef::Float(1.25));
        assert_eq!(properties[2].1, MvtValueRef::Double(2.5));
        assert_eq!(properties[3].1, MvtValueRef::Int(-3));
        assert_eq!(properties[4].1, MvtValueRef::UInt(4));
        assert_eq!(properties[5].1, MvtValueRef::SInt(-5));
        assert_eq!(properties[6].1, MvtValueRef::Bool(true));
        assert_eq!(properties[7].1, MvtValueRef::Null);
        assert_eq!(
            properties[0].1.into_owned(),
            MvtValue::String("name".into())
        );
        assert_eq!(properties[7].1.into_owned(), MvtValue::Null);

        let Geometry::MultiPoint(points) = feature.geometry().unwrap() else {
            panic!("expected multipoint");
        };
        assert_eq!(points.0.len(), 2);
        assert!(matches!(
            feature.geometry().unwrap(),
            Geometry::MultiPoint(_)
        ));
    }

    #[test]
    fn invalid_versions_and_geometry_types_are_errors() {
        let layer = proto_tile::Layer {
            version: 4,
            name: "bad".into(),
            ..Default::default()
        };
        let bytes = encode_layer(layer);
        assert!(matches!(
            MvtReaderRef::new(&bytes),
            Err(MvtError::UnsupportedVersion { version: 4, .. })
        ));

        let feature = proto_tile::Feature {
            r#type: Some(proto_tile::GeomType::Unknown),
            geometry: vec![9, 0, 0],
            ..Default::default()
        };
        let mut layer = proto_tile::Layer {
            version: 2,
            name: "geometry".into(),
            ..Default::default()
        };
        layer.features = vec![feature];
        let bytes = encode_layer(layer);
        let reader = MvtReaderRef::new(&bytes).unwrap();
        let feature = first_feature(&reader);
        assert!(matches!(feature.geometry(), Err(MvtError::InvalidGeometry)));
    }
}
