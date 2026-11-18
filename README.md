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

### Decoding only what you read

`MvtReaderRef` decodes the whole tile up front. `MvtLazyReader` only finds the
layers: a layer is decoded when you ask for it, and a feature when you read it.
Its tags and geometry commands are read from the tile's bytes as you iterate
them, so a reader that skips layers, or streams the commands into its own
geometry, allocates only each layer's key and value tables:

```rust
use fast_mvt::{MvtLazyReader, MvtResult};

fn count_vertices(bytes: &[u8], layer_name: &str) -> MvtResult<usize> {
    let reader = MvtLazyReader::new(bytes)?;
    let Some(entry) = reader.layers().find(|entry| entry.name() == layer_name) else {
        return Ok(0);
    };
    let layer = entry.decode()?;
    let mut vertices = 0;
    for feature in layer.features() {
        // MoveTo and LineTo commands are followed by two parameters per vertex.
        vertices += feature?.geometry_commands().len() / 2;
    }
    Ok(vertices)
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
structure. It nests tile → layer → feature and unwinds in reverse:
`feature.end()` returns the layer, `layer.end()` returns the tile, then
`tile.encode()` produces the bytes:

```rust
use fast_mvt::{MvtGeometry, MvtResult, MvtTileBuilder};

fn write_tile() -> MvtResult<Vec<u8>> {
    let tile = MvtTileBuilder::new();

    let mut feature = tile
        .layer("places")?
        .feature(&MvtGeometry::Point((1, 2).into()))?;
    feature.id(Some(7));
    feature.tag("name", "Example")?;
    let tile = feature.end().end();

    // The tile takes as many layers as you like.
    let feature = tile
        .layer("landmarks")?
        .feature(&MvtGeometry::Point((5, 6).into()))?;
    Ok(feature.end().end().encode())
}
```

### Writing straight from your own data

Features are written to the tile bytes as they end, and the builders reuse their
buffers, so a layer of any size costs no allocation per feature. To keep it that
way when your data is not a `geo_types` geometry, start a feature with
`feature_of()` and add its geometry from coordinates with `points()`, `line()` or
`ring()`. `tag_ref()` takes a borrowed key and value, and allocates only for a key
or value the layer has not seen yet:

```rust
use fast_mvt::{MvtCoord, MvtGeomType, MvtLayerBuilder, MvtResult, MvtValueRef};

fn write_square(name: &str) -> MvtResult<Vec<u8>> {
    let square = [(0, 0), (10, 0), (10, 10), (0, 10)].map(|(x, y)| MvtCoord { x, y });
    let mut feature = MvtLayerBuilder::new("parks")?.feature_of(MvtGeomType::Polygon);
    // A polygon is its exterior ring followed by its holes; rings are rewound as needed.
    feature.ring(square, true)?;
    feature.tag_ref("name", MvtValueRef::String(name))?;
    Ok(feature.end().encode())
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

Measured on an Intel Core i9-10885H, x86-64 Linux, with Rust 1.100.

#### Decoding

Run with `just bench-decode`:

| Decoder                         | Time     | Throughput  | Compare     |
|---------------------------------|----------|-------------|-------------|
| `fast-mvt`                      | 80.5 ms  | 191.4 MiB/s | -           |
| `fast-mvt`, lazy                | 63.3 ms  | 243.4 MiB/s | 1.3x faster |
| `tinymvt 0.3.0`                 | 192.9 ms | 79.9 MiB/s  | 2.4x slower |
| `mvt-reader 2.3.0`              | 597.3 ms | 25.8 MiB/s  | 7.4x slower |
| `mvt` <br/>decode not supported | n/a      | n/a         | n/a         |

#### Encoding

Run with `just bench-encode`:

Encoding from an already parsed integer tile model:

| Encoder                                | Time    | Throughput  | Compare     |
|----------------------------------------|---------|-------------|-------------|
| `fast-mvt`                             | 4.4 ms  | 194.4 MiB/s | -           |
| `tinymvt 0.3.0`                        | 14.2 ms | 60.3 MiB/s  | 3.2x slower |
| `mvt 0.15.0`                           | 23.7 ms | 36.2 MiB/s  | 5.4x slower |
| `mvt-reader` <br/>encode not supported | n/a     | n/a         | n/a         |

Encoding from an owned tile value. Note that "owned" benchmark includes deep-cloning of each tile, so it makes no sense to compare throughput between the owned vs referenced table above, only between different encoders.

| Encoder                                | Time    | Throughput | Compare     |
|----------------------------------------|---------|------------|-------------|
| `fast-mvt`                             | 11.5 ms | 74.8 MiB/s | -           |
| `tinymvt 0.3.0`                        | 23.2 ms | 37.0 MiB/s | 2.0x slower |
| `mvt 0.15.0`                           | 32.2 ms | 26.6 MiB/s | 2.8x slower |
| `mvt-reader` <br/>encode not supported | n/a     | n/a        | n/a         |

#### CPU instructions

Run with `just bench-cpu`, which needs Valgrind and `gungraun-runner`. It counts the CPU instructions of the same
work as the tables above with Callgrind, which is deterministic, unlike the time.

| Decoder            | Instructions | Compare   |
|--------------------|--------------|-----------|
| `fast-mvt`         | 593 M        | -         |
| `fast-mvt`, lazy   | 640 M        | 1.1x more |
| `tinymvt 0.3.0`    | 1,703 M      | 2.9x more |
| `mvt-reader 2.3.0` | 4,454 M      | 7.5x more |

| Encoder         | From a parsed tile | Compare   | From an owned tile | Compare   |
|-----------------|--------------------|-----------|--------------------|-----------|
| `fast-mvt`      | 37.5 M             | -         | 107.2 M            | -         |
| `tinymvt 0.3.0` | 115.3 M            | 3.1x more | 185.0 M            | 1.7x more |
| `mvt 0.15.0`    | 202.2 M            | 5.4x more | 271.7 M            | 2.5x more |

## Features

| Feature     | Purpose                                                                 |
|-------------|-------------------------------------------------------------------------|
| `reader`    | MVT tile decoding from bytes.                                           |
| `writer`    | MVT tile encoding into bytes.                                           |
| `json`      | Enable serde JSON support.                                              |
| `arbitrary` | Derive `arbitrary::Arbitrary` for generated protobuf types for fuzzing. |

The generated protobuf files are checked in, and no build runs `protoc`. Run
`just update-generated` (it needs `protoc`) to regenerate them with `examples/codegen.rs`
after `buffa` upgrades; CI's `just codegen-check` fails if they are stale.

## Development

* This project is easier to develop with [just](https://github.com/casey/just#readme), a modern alternative to `make`.
  Install it with `cargo install just`.
* To get a list of available commands, run `just`.
* To run tests, use `just test`.
* Copy/paste detection uses [jscpd](https://github.com/kucherenko/jscpd), configured in [`.jscpd.json`](.jscpd.json).
  Run `just cpd` locally; on pull requests, CI posts a summary comment highlighting newly added clones.

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
