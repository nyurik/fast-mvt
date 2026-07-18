# fast-mvt

[![GitHub repo](https://img.shields.io/badge/github-nyurik/fast--mvt-8da0cb?logo=github)](https://github.com/nyurik/fast-mvt)
[![crates.io version](https://img.shields.io/crates/v/fast-mvt)](https://crates.io/crates/fast-mvt)
[![crate usage](https://img.shields.io/crates/d/fast-mvt)](https://crates.io/crates/fast-mvt)
[![docs.rs status](https://img.shields.io/docsrs/fast-mvt)](https://docs.rs/fast-mvt)
[![crates.io license](https://img.shields.io/crates/l/fast-mvt)](https://github.com/nyurik/fast-mvt/blob/main/LICENSE-APACHE)
[![CI build status](https://github.com/nyurik/fast-mvt/actions/workflows/ci.yml/badge.svg)](https://github.com/nyurik/fast-mvt/actions)
[![Codecov](https://img.shields.io/codecov/c/github/nyurik/fast-mvt)](https://app.codecov.io/gh/nyurik/fast-mvt)

`fast-mvt` is an integer-only Mapbox Vector Tile reader and writer for Rust.
Geometry uses `geo-types` with `i32` coordinates. The crate does not project,
scale, round, or handle floating point geometry coordinates; callers provide and
receive tile-space integers.

## Installation

Install the `mvt` CLI from prebuilt release binaries with
[cargo-binstall](https://github.com/cargo-bins/cargo-binstall):

```bash
cargo binstall fast-mvt
```

You can also build it from source:

```bash
cargo install fast-mvt --features cli --bin mvt
```

## Decoding a tile

```rust
use fast_mvt::{MvtReaderRef, MvtResult};

fn read_tile(bytes: &[u8]) -> MvtResult<()> {
    let reader = MvtReaderRef::new(bytes)?;

    for layer in reader.layers() {
        for feature in layer.features() {
            let geometry = feature.geometry()?;
            println!("geo: {geometry:?}");
            let id = feature.id();
            println!("id: {id:?}");

            for property in feature.properties() {
                let (key, value) = property?;
                println!("{key} = {value:?}");
            }
        }
    }

    Ok(())
}
```

## Encoding a tile

```rust
use fast_mvt::{MvtGeometry, MvtResult, MvtTileBuilder};

fn write_tile() -> MvtResult<Vec<u8>> {
    let tile = MvtTileBuilder::new();
    let layer = tile.layer("places")?;

    let mut feature = layer.feature(&MvtGeometry::Point((1, 2).into()))?;
    feature.id(Some(7));
    feature.tag("name", "Example")?;
    feature.tag("visible", true)?;
    let layer = feature.end();

    let tile = layer.end();
    Ok(tile.encode())
}
```

Opening a layer consumes the tile builder, and opening a feature consumes the
layer builder. `end()` returns the parent builder with the child committed, so
there is no reachable partially committed layer or tile while a child is in
progress. `MvtTileBuilder::encode()` produces the final tile bytes. A single-layer tile byte buffer is also a framed layer chunk, so
multiple independently built layer buffers can be concatenated to form a tile.

## Parallel encoding

Key and value deduplication is scoped to a single layer, so layers can be built
completely independently — one per thread.
`MvtLayerBuilder::new()` builds a standalone layer and `encode()` returns its
framed bytes; concatenate the buffers (in whatever order you want the layers) to
form the final tile.

```rust
use fast_mvt::{MvtGeometry, MvtLayerBuilder, MvtResult};

fn encode_tile(layers: &[(&str, MvtGeometry, &str)]) -> MvtResult<Vec<u8>> {
    let buffers: Vec<Vec<u8>> = layers
        // This code is single-threaded, but it is easy to parallelize
        // with the `rayon` crate: swap `.iter()` for `.par_iter()` to encode
        // the layers in parallel.
        .iter()
        .map(|(name, geom, prop)| {
            let mut feature = MvtLayerBuilder::new(*name)?.feature(geom)?;
            feature.tag("property", *prop)?;
            Ok(feature.end().encode())
        })
        .collect::<MvtResult<_>>()?;

    // Concatenate the framed layer buffers, in order, into a tile.
    Ok(buffers.concat())
}
```

## Benchmarks

#### Decoding

Run with `just bench-decode`:

| Decoder                         | Time     | Throughput  | Compare     |
|---------------------------------|----------|-------------|-------------|
| `fast-mvt`                      | 97.6 ms  | 157.9 MiB/s | -           |
| `tinymvt 0.3.0`                 | 192.3 ms | 80.2 MiB/s  | 2.0x slower |
| `mvt-reader 2.3.0`              | 597.0 ms | 25.8 MiB/s  | 6.1x slower |
| `mvt` <br/>decode not supported | n/a      | n/a         | n/a         |

#### Encoding

Run with `just bench-encode`:

Encoding from an already parsed integer tile model:

| Encoder                                | Time    | Throughput | Compare     |
|----------------------------------------|---------|------------|-------------|
| `fast-mvt`                             | 12.9 ms | 66.5 MiB/s | -           |
| `tinymvt 0.3.0`                        | 14.1 ms | 60.9 MiB/s | 1.1x slower |
| `mvt 0.14.0`                           | 23.4 ms | 36.7 MiB/s | 1.8x slower |
| `mvt-reader` <br/>encode not supported | n/a     | n/a        | n/a         |

Encoding from an owned tile value. Note that "owned" benchmark includes deep-cloning of each tile, so it makes no sense to compare throughput between the owned vs referenced table above, only between different encoders. Owned path is usually better.

| Encoder                                | Time    | Throughput | Compare     |
|----------------------------------------|---------|------------|-------------|
| `fast-mvt`                             | 18.6 ms | 46.0 MiB/s | -           |
| `tinymvt 0.3.0`                        | 23.1 ms | 37.1 MiB/s | 1.2x slower |
| `mvt 0.14.0`                           | 32.9 ms | 26.1 MiB/s | 1.8x slower |
| `mvt-reader` <br/>encode not supported | n/a     | n/a        | n/a         |

## Features

| Feature     | Purpose                                                                 |
|-------------|-------------------------------------------------------------------------|
| `reader`    | MVT tile decoding from bytes.                                           |
| `writer`    | MVT tile encoding into bytes.                                           |
| `json`      | Enable serde JSON support.                                              |
| `codegen`   | Regenerate checked-in protobuf bindings from `src/vector_tile.proto`.   |
| `arbitrary` | Derive `arbitrary::Arbitrary` for generated protobuf types for fuzzing. |

The generated protobuf files are checked in, so normal builds do not require
`protoc`. Run `just update-generated` to refresh generated code after `buffa` upgrades.

## Development

* This project is easier to develop with [just](https://github.com/casey/just#readme), a modern alternative to `make`.
  Install it with `cargo install just`.
* To get a list of available commands, run `just`.
* To run tests, use `just test`.

## Credits

The code was inspired by several open source MVT implementations:

- Encoding and testing from `mvt` crate by the Minnesota Department of Transportation.
- Decoding and testing from `mvt-reader` by Paul Lange.

## License

Licensed under either of

* Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or <https://www.apache.org/licenses/LICENSE-2.0>)
* MIT license ([LICENSE-MIT](LICENSE-MIT) or <https://opensource.org/licenses/MIT>)
  at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in the work by you, as defined in the
Apache-2.0 license, shall be dual-licensed as above, without any
additional terms or conditions.
