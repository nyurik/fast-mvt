use std::fmt;
use std::num::NonZeroU32;

use super::feature::MvtFeatureRef;
use super::property::{MvtValueRef, value_ref};
use crate::generated::vector_tile::tile as proto_tile;
use crate::{DEFAULT_EXTENT, MvtError, MvtLayer, MvtResult};

#[derive(Copy, Clone)]
pub struct MvtLayerRef<'a>(&'a proto_tile::LayerView<'a>);

impl<'a> MvtLayerRef<'a> {
    pub(crate) fn new(view: &'a proto_tile::LayerView<'a>) -> Self {
        Self(view)
    }

    #[must_use]
    pub fn name(self) -> &'a str {
        self.0.name
    }

    #[must_use]
    pub fn version(self) -> u32 {
        self.0.version
    }

    #[must_use]
    pub fn extent(self) -> u32 {
        self.0.extent.unwrap_or(DEFAULT_EXTENT.get())
    }

    #[must_use]
    pub fn feature_count(self) -> usize {
        self.0.features.len()
    }

    #[must_use]
    pub fn is_empty(self) -> bool {
        self.0.features.is_empty()
    }

    #[must_use]
    pub fn keys(self) -> &'a [&'a str] {
        &self.0.keys
    }

    #[must_use]
    pub fn values(self) -> impl ExactSizeIterator<Item = MvtValueRef<'a>> {
        self.0.values.iter().map(value_ref)
    }

    #[must_use]
    pub fn features(self) -> impl ExactSizeIterator<Item = MvtFeatureRef<'a>> {
        self.0
            .features
            .iter()
            .map(move |feature| MvtFeatureRef::new(self.0, feature))
    }

    pub fn to_layer(self) -> MvtResult<MvtLayer> {
        let extent = match self.0.extent {
            Some(extent) => NonZeroU32::new(extent).ok_or(MvtError::InvalidExtent)?,
            None => DEFAULT_EXTENT,
        };
        self.features()
            .map(MvtFeatureRef::to_feature)
            .collect::<MvtResult<Vec<_>>>()
            .map(|features| MvtLayer {
                name: self.0.name.to_string(),
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
        for (index, feature) in self.features().enumerate() {
            writeln!(f, "  feature: {index}")?;
            // Each feature's `Debug` block is already newline-terminated.
            write!(f, "{feature:?}")?;
        }
        Ok(())
    }
}
