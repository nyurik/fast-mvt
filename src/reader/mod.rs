mod feature;
mod layer;
mod property;
mod tile;

pub use feature::MvtFeatureRef;
pub use layer::MvtLayerRef;
pub use property::{MvtPropertyIter, MvtValueRef};
pub use tile::MvtReaderRef;

#[cfg(test)]
mod tests {
    use buffa::Message as _;

    use super::MvtReaderRef;
    use crate::generated::vector_tile::{Tile, tile as proto_tile};

    /// Encodes a single-layer tile and returns a reader over leaked bytes, so
    /// tests can borrow it for `'static`.
    #[allow(clippy::disallowed_methods)]
    pub fn reader_from_layer(layer: proto_tile::Layer) -> MvtReaderRef<'static> {
        let bytes = Tile {
            layers: vec![layer],
        }
        .encode_to_vec();
        let bytes = Box::leak(bytes.into_boxed_slice());
        MvtReaderRef::new(bytes).unwrap()
    }
}
