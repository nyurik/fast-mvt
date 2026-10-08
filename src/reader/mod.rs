mod feature;
mod layer;
mod property;
mod tile;

pub use feature::MvtFeatureRef;
pub use layer::MvtLayerRef;
pub use property::MvtPropertyIter;
pub use tile::MvtReaderRef;

#[cfg(test)]
mod tests {
    use buffa::Message as _;

    use super::{MvtFeatureRef, MvtReaderRef};
    use crate::generated::vector_tile::{Tile, tile as proto_tile};

    /// Encodes a single-layer tile. Tests keep the returned bytes alive and
    /// borrow a [`MvtReaderRef`] from them, so nothing needs to be leaked.
    pub fn encode_layer(layer: proto_tile::Layer) -> Vec<u8> {
        Tile {
            layers: vec![layer],
        }
        .encode_to_vec()
    }

    /// Encodes a single-layer tile whose layer carries `feature` as its sole feature.
    pub fn encode_feature(mut layer: proto_tile::Layer, feature: proto_tile::Feature) -> Vec<u8> {
        layer.features = vec![feature];
        encode_layer(layer)
    }

    /// The first feature of the first layer, for tests that only need one.
    pub fn first_feature<'r>(reader: &'r MvtReaderRef<'_>) -> MvtFeatureRef<'r> {
        reader.layers().next().unwrap().features().next().unwrap()
    }
}
