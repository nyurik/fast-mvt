use std::fmt;
use std::num::NonZeroU32;

use super::feature::MvtFeatureRef;
use super::wire::{LayerData, TileData};
use crate::{DEFAULT_EXTENT, MvtError, MvtLayer, MvtResult, MvtValueRef};

#[derive(Copy, Clone)]
pub struct MvtLayerRef<'a> {
    tile: &'a TileData<'a>,
    layer: &'a LayerData<'a>,
}

impl<'a> MvtLayerRef<'a> {
    pub(crate) fn new(tile: &'a TileData<'a>, layer: &'a LayerData<'a>) -> Self {
        Self { tile, layer }
    }

    #[must_use]
    pub fn name(self) -> &'a str {
        self.layer.name
    }

    #[must_use]
    pub fn version(self) -> u32 {
        self.layer.version
    }

    #[must_use]
    pub fn extent(self) -> u32 {
        self.layer.extent.unwrap_or(DEFAULT_EXTENT.get())
    }

    #[must_use]
    pub fn feature_count(self) -> usize {
        self.tile.features(self.layer).len()
    }

    #[must_use]
    pub fn is_empty(self) -> bool {
        self.tile.features(self.layer).is_empty()
    }

    #[must_use]
    pub fn keys(self) -> &'a [&'a str] {
        self.tile.keys(self.layer)
    }

    pub fn values(self) -> impl ExactSizeIterator<Item = MvtValueRef<'a>> {
        self.tile.values(self.layer).iter().copied()
    }

    pub fn features(self) -> impl ExactSizeIterator<Item = MvtFeatureRef<'a>> {
        self.tile
            .features(self.layer)
            .iter()
            .map(move |feature| MvtFeatureRef::new(self.tile, self.layer, feature))
    }

    pub fn to_layer(self) -> MvtResult<MvtLayer> {
        let extent = match self.layer.extent {
            Some(extent) => NonZeroU32::new(extent).ok_or(MvtError::InvalidExtent)?,
            None => DEFAULT_EXTENT,
        };
        let mut features = Vec::with_capacity(self.feature_count());
        for feature in self.features() {
            features.push(feature.to_feature()?);
        }
        Ok(MvtLayer {
            name: self.layer.name.to_string(),
            extent,
            features,
        })
    }
}

impl fmt::Debug for MvtLayerRef<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "  name: {}", self.name())?;
        writeln!(f, "  version: {}", self.version())?;
        writeln!(f, "  extent: {}", self.extent())?;
        if !keys_follow_first_use(*self) {
            writeln!(f, "  key table: {:?}", self.keys())?;
        }
        for (index, feature) in self.features().enumerate() {
            writeln!(f, "  feature: {index}")?;
            // Each feature's `Debug` block is already newline-terminated.
            write!(f, "{feature:?}")?;
        }
        Ok(())
    }
}

/// Whether the key table is just the keys the features use, in the order they first use them,
/// which the feature dumps already show.
fn keys_follow_first_use(layer: MvtLayerRef<'_>) -> bool {
    let mut next = 0;
    for feature in layer.features() {
        for key in feature.tag_indices().step_by(2) {
            let key = key as usize;
            if key > next {
                return false;
            }
            if key == next {
                next += 1;
            }
        }
    }
    next == layer.keys().len()
}

#[cfg(test)]
mod tests {
    use buffa::Message as _;

    use crate::MvtReaderRef;
    use crate::generated::vector_tile::{Tile, tile as proto_tile};

    #[test]
    fn debug_shows_a_key_table_in_another_order_than_the_properties() {
        let layer = |name: &str, keys: [&str; 2], tags: Vec<u32>| proto_tile::Layer {
            version: 2,
            name: name.into(),
            keys: keys.map(String::from).to_vec(),
            values: vec![
                proto_tile::Value {
                    string_value: Some("x".into()),
                    ..Default::default()
                },
                proto_tile::Value {
                    string_value: Some("y".into()),
                    ..Default::default()
                },
            ],
            features: vec![proto_tile::Feature {
                r#type: Some(proto_tile::GeomType::Point),
                geometry: vec![9, 0, 0],
                tags,
                ..Default::default()
            }],
            ..Default::default()
        };
        let bytes = Tile {
            layers: vec![
                layer("reordered", ["b", "a"], vec![1, 0, 0, 1]),
                layer("in_order", ["a", "b"], vec![0, 0, 1, 1]),
            ],
        }
        .encode_to_vec();
        let reader = MvtReaderRef::new(&bytes).unwrap();
        insta::assert_debug_snapshot!(reader, @r#"
        layer: 0
          name: reordered
          version: 2
          extent: 4096
          key table: ["b", "a"]
          feature: 0
            id: (none)
            geometry: POINT(0,0)
            properties:
              a = "x"
              b = "y"
        =============================================================
        layer: 1
          name: in_order
          version: 2
          extent: 4096
          feature: 0
            id: (none)
            geometry: POINT(0,0)
            properties:
              a = "x"
              b = "y"
        "#);
    }
}
