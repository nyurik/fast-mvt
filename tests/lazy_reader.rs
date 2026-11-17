#![cfg(feature = "reader")]

//! The lazy reader reads every tile the eager reader reads, the same way.

use std::fs;
use std::path::Path;

use fast_mvt::{MvtLazyReader, MvtReaderRef, MvtResult};
use test_each_file::test_each_path;

test_each_path! { for ["mvt"] in "fixtures/mvt-fixtures/fixtures" as fixtures => reads_as_eager }
test_each_path! { for ["mvt"] in "fixtures/mvt-fixtures/real-world" as real_world => reads_as_eager }

fn reads_as_eager([path]: [&Path; 1]) {
    let data = fs::read(path).expect("read MVT file");
    let Ok(eager) = MvtReaderRef::new(&data) else {
        // The lazy reader decodes less, so it may read a tile the eager
        // reader rejects.
        return;
    };
    let lazy = MvtLazyReader::new(&data).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    assert_eq!(
        lazy.layer_count(),
        eager.layer_count(),
        "{}",
        path.display()
    );
    for (entry, eager) in lazy.layers().zip(eager.layers()) {
        assert_eq!(entry.name(), eager.name());
        let layer = entry
            .decode()
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        assert_eq!(layer.version(), eager.version());
        assert_eq!(layer.extent(), eager.extent());
        assert_eq!(layer.keys(), eager.keys());
        assert!(layer.values().iter().copied().eq(eager.values()));
        assert_eq!(layer.feature_count(), eager.feature_count());
        for (lazy, eager) in layer.features().zip(eager.features()) {
            let lazy = lazy.unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            assert_eq!(lazy.id(), eager.id());
            assert_eq!(lazy.geom_type(), eager.geom_type());
            assert_eq!(
                lazy.tags().collect::<MvtResult<Vec<_>>>().expect("tags"),
                eager.tags()
            );
            assert_eq!(
                lazy.geometry_commands()
                    .collect::<MvtResult<Vec<_>>>()
                    .expect("geometry"),
                eager.geometry_commands()
            );
            let lazy_properties = lazy.properties().map(|p| p.map_err(|e| e.to_string()));
            let eager_properties = eager.properties().map(|p| p.map_err(|e| e.to_string()));
            assert!(lazy_properties.eq(eager_properties), "{}", path.display());
            assert_eq!(
                lazy.geometry().map_err(|e| e.to_string()),
                eager.geometry().map_err(|e| e.to_string())
            );
        }
    }
}
