//! Regenerates the checked-in protobuf bindings in `src/generated` from `src/vector_tile.proto`.
//!
//! Run it with `just update-generated`; it needs `protoc`. CI runs `just codegen-check`, which fails if the result
//! differs from the checked-in code.

use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    buffa_build::Config::new()
        .files(&[root.join("src/vector_tile.proto")])
        .includes(&[root.join("src")])
        .out_dir(root.join("src/generated"))
        .include_file("mod.rs")
        .generate_json(true)
        .generate_arbitrary(true)
        .gate_impls_on_crate_features(true)
        .preserve_unknown_fields(false)
        .views_feature_name("reader")
        .packed_views(true)
        .compile()?;
    Ok(())
}
