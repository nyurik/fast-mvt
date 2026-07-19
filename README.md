# fast-mvt

[![GitHub repo](https://img.shields.io/badge/github-nyurik/fast--mvt-8da0cb?logo=github)](https://github.com/nyurik/fast-mvt)
[![crates.io version](https://img.shields.io/crates/v/fast-mvt)](https://crates.io/crates/fast-mvt)
[![crate usage](https://img.shields.io/crates/d/fast-mvt)](https://crates.io/crates/fast-mvt)
[![docs.rs status](https://img.shields.io/docsrs/fast-mvt)](https://docs.rs/fast-mvt)
[![crates.io license](https://img.shields.io/crates/l/fast-mvt)](https://github.com/nyurik/fast-mvt/blob/main/LICENSE-APACHE)
[![CI build status](https://github.com/nyurik/fast-mvt/actions/workflows/ci.yml/badge.svg)](https://github.com/nyurik/fast-mvt/actions)
[![Codecov](https://img.shields.io/codecov/c/github/nyurik/fast-mvt)](https://app.codecov.io/gh/nyurik/fast-mvt)

`fast-mvt` is an integer-only Mapbox Vector Tile reader and writer for Rust.
Geometry uses [`geo-types`](https://docs.rs/geo-types/latest/geo_types/) with `i32` coordinates. The crate does not project,
scale, round, or handle floating point geometry coordinates; callers provide and
receive tile-space integers. See [`geo`](https://docs.rs/geo/latest/geo/) for the geometry manipulations you may need.

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

The simplest path starts a layer with `MvtLayerBuilder::new()`, adds features and
their tags, and produces the tile bytes with `MvtLayerBuilder::encode()`. The
builder is a consuming chain — each step returns the next builder and `end()`
returns the parent — so a half-built layer or feature can never be used by
accident.

```rust
use fast_mvt::{MvtGeometry, MvtLayerBuilder, MvtResult};

fn create_tile() -> MvtResult<Vec<u8>> {
    let layer = MvtLayerBuilder::new("places")?;

    let mut feature = layer.feature(&MvtGeometry::Point((1, 2).into()))?;
    feature.id(Some(7));
    feature.tag("name", "Example")?;
    feature.tag("visible", true)?;

    // `feature.end()` commits the feature and returns the layer;
    // `layer.encode()` produces the final tile bytes.
    Ok(feature.end().encode())
}
```

To add more layers, chain `MvtLayerBuilder::layer()`, which commits the current
layer and opens the next one — without exposing the underlying tile builder:

```rust
use fast_mvt::{MvtGeometry, MvtLayerBuilder, MvtResult};

fn create_tile() -> MvtResult<Vec<u8>> {
    let mut feature = MvtLayerBuilder::new("roads")?
        .feature(&MvtGeometry::Point((1, 2).into()))?;
    feature.tag("kind", "residential")?;

    let mut feature = feature.end().layer("water")?
        .feature(&MvtGeometry::Point((3, 4).into()))?;
    feature.tag("name", "Lake")?;

    Ok(feature.end().encode())
}
```

`MvtTileBuilder` is the underlying root, useful when you want a tile-first
structure or need to preallocate. It nests tile → layer → feature and unwinds in
reverse: `feature.end()` returns the layer, `layer.end()` returns the tile, then
`tile.encode()` produces the bytes. The `with_capacity` constructors preallocate
for a known number of layers or features:

```rust
use fast_mvt::{MvtGeometry, MvtResult, MvtTileBuilder};

fn write_tile() -> MvtResult<Vec<u8>> {
    let tile = MvtTileBuilder::with_capacity(1);
    let layer = tile.layer_with_capacity("places", 1)?;

    let mut feature = layer.feature(&MvtGeometry::Point((1, 2).into()))?;
    feature.id(Some(7));
    feature.tag("name", "Example")?;
    feature.tag("visible", true)?;

    Ok(feature.end().end().encode())
}
```

## Parallel encoding

Key and value deduplication is scoped to a single layer, so layers can be built
completely independently. Each `MvtLayerBuilder::encode()` output is a framed
layer chunk, and concatenating those chunks — in whatever order you want the
layers — forms a complete tile. Per-layer parallelism is therefore
straightforward: encode each layer on its own thread, then concatenate the
buffers.

```rust
use fast_mvt::{MvtGeometry, MvtLayerBuilder, MvtResult};

fn encode_layer(name: &str, geometry: &MvtGeometry) -> MvtResult<Vec<u8>> {
    let feature = MvtLayerBuilder::new(name)?.feature(geometry)?;
    Ok(feature.end().encode())
}

fn encode_tile(layers: &[(&str, MvtGeometry)]) -> MvtResult<Vec<u8>> {
    let buffers: Vec<Vec<u8>> = layers
        // This code is single-threaded, but it is easy to parallelize
        // with the `rayon` crate: swap `.iter()` for `.par_iter()` to encode
        // the layers in parallel.
        .iter()
        .map(|(name, geometry)| encode_layer(name, geometry))
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
