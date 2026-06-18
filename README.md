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

    let mut feature = layer.feature(MvtGeometry::Point((1, 2).into()))?;
    feature.id(Some(7));
    feature.tag("name", "Example")?;
    feature.tag("visible", true)?;
    let layer = feature.finish();

    let tile = layer.finish();
    Ok(tile.finish())
}
```

Opening a layer consumes the tile builder, and opening a feature consumes the
layer builder. Finishing returns the parent builder with the child committed, so
there is no reachable partially committed layer or tile while a child is in
progress. A single-layer tile byte buffer is also a framed layer chunk, so
multiple independently built layer buffers can be concatenated to form a tile.

## Benchmarks

#### Decoding

Run with `just bench-decode`:

| Decoder      |    Time |   Throughput |     Compare |
|--------------|--------:|-------------:|------------:|
| `fast-mvt`   |  108 ms |  142.9 MiB/s |           - |
| `tinymvt`    |  196 ms |   78.5 MiB/s | 1.8x slower |
| `mvt-reader` |  622 ms |   24.8 MiB/s | 5.8x slower |

#### Encoding

Run with `just bench-encode`:

Encoding from an already parsed integer tile model:

| Encoder    |     Time | Throughput |      Compare |
|------------|---------:|-----------:|-------------:|
| `fast-mvt` |  13.4 ms | 63.8 MiB/s |            - |
| `tinymvt`  |  14.8 ms | 58.0 MiB/s |  1.1x slower |
| `mvt`      |  25.2 ms | 34.1 MiB/s | 1.9x slower* |

Encoding from an owned tile value. Note that "owned" benchmark includes deep-cloning of each tile, so it makes no sense to compare throughput between the owned vs referenced table above, only between different encoders. Owned path is usually better.

| Encoder    |     Time | Throughput |      Compare |
|------------|---------:|-----------:|-------------:|
| `fast-mvt` |  19.3 ms | 44.3 MiB/s |            - |
| `tinymvt`  |  24.4 ms | 35.2 MiB/s |  1.3x slower |
| `mvt`      |  34.3 ms | 25.0 MiB/s | 1.8x slower* |

\* The `mvt` encoder is broken for large real-world tiles, resulting in 100x slower performance.

## Features

| Feature     | Purpose                                                                 |
|-------------|-------------------------------------------------------------------------|
| `reader`    | MVT tile decoding from bytes.                                           |
| `writer`    | MVT tile encoding into bytes.                                           |
| `json`      | Enable serde JSON support.                                              |
| `codegen`   | Regenerate checked-in protobuf bindings from `src/vector_tile.proto`.   |
| `arbitrary` | Derive `arbitrary::Arbitrary` for generated protobuf types for fuzzing. |
| `views`     | Internal feature, do not use. Must be here due to buffa limitations.    |

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
