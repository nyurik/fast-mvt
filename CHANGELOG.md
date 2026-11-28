# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.8.2](https://github.com/nyurik/fast-mvt/compare/v0.8.1...v0.8.2) - 2026-11-28

### Other

- update Cargo.lock dependencies

## [0.8.1](https://github.com/nyurik/fast-mvt/compare/v0.8.0...v0.8.1) - 2026-11-18

### Other

- *(reader)* inline the small per-feature and per-tag accessors ([#78](https://github.com/nyurik/fast-mvt/pull/78))

## [0.8.0](https://github.com/nyurik/fast-mvt/compare/v0.7.3...v0.8.0) - 2026-11-18

Decoding runs about 30% fewer CPU instructions and keeps 71% less heap; encoding runs about 40% fewer, with
unchanged output.

Contributors: [Anthony Krupko](https://github.com/AnthonyKrupko) and [Yuri Astrakhan](https://github.com/nyurik).

### Changed

- *(reader)* [**breaking**] read tiles in one pass, without per-feature allocations
  ([#61](https://github.com/nyurik/fast-mvt/pull/61)): 32% fewer instructions for a full traversal, 26% for the
  lazy reader
- *(writer)* reuse layer state and intern keys and values as their encoded fields
  ([#76](https://github.com/nyurik/fast-mvt/pull/76), building on [#62](https://github.com/nyurik/fast-mvt/pull/62))
- *(writer)* write geometry as varints while it is built ([#74](https://github.com/nyurik/fast-mvt/pull/74))
- *(writer)* write one-count command words as their single byte ([#77](https://github.com/nyurik/fast-mvt/pull/77))

### Breaking changes

`tags()` and `geometry_commands()` return an `ExactSizeIterator<Item = u32>` in both readers, and
`MvtLazyPropertyIter` is replaced by `MvtPropertyIter`, which now has two lifetimes:

```rust
// 0.7
let tags: &[u32] = feature.tags();
let commands = lazy_feature.geometry_commands().collect::<MvtResult<Vec<u32>>>()?;

// 0.8
let tags: Vec<u32> = feature.tags().collect();
let commands: Vec<u32> = lazy_feature.geometry_commands().collect();
```

### Fixed

- build with Rust 1.100 without an unused-dependency warning ([#70](https://github.com/nyurik/fast-mvt/pull/70))

### Other

- add the `mvt_reader` fuzz target ([#61](https://github.com/nyurik/fast-mvt/pull/61))
- drop the fixture tiles outside the CPU instruction counts ([#75](https://github.com/nyurik/fast-mvt/pull/75))
- refresh the README benchmark tables
- update dependencies and sort imports ([#73](https://github.com/nyurik/fast-mvt/pull/73))

## [0.7.3](https://github.com/nyurik/fast-mvt/compare/v0.7.2...v0.7.3) - 2026-11-12

### Added

- *(reader)* add a lazy reader that decodes only what is read ([#68](https://github.com/nyurik/fast-mvt/pull/68))

## [0.7.2](https://github.com/nyurik/fast-mvt/compare/v0.7.1...v0.7.2) - 2026-11-08

### Other

- [**breaking**] regenerate protobuf code with an example ([#65](https://github.com/nyurik/fast-mvt/pull/65))

## [0.7.1](https://github.com/nyurik/fast-mvt/compare/v0.7.0...v0.7.1) - 2026-10-26

### Added

- *(writer)* `MvtTileBuilder::preset_keys` starts the next layer with keys no feature has to use ([#60](https://github.com/nyurik/fast-mvt/pull/60))

### Other

- ignore codecov fail for projects

## [0.7.0](https://github.com/nyurik/fast-mvt/compare/v0.6.2...v0.7.0) - 2026-10-08

### Added

- *(writer)* [**breaking**] stream features into the tile bytes, from borrowed input ([#53](https://github.com/nyurik/fast-mvt/pull/53))
  - Features are written straight into the tile bytes as they end, so a layer costs no allocation per
    feature. Output is byte-identical to before; encoding allocates about 90% less and runs about 16%
    fewer instructions from a borrowed tile.
  - New: `MvtLayerBuilder::feature_of(MvtGeomType)` starts a feature whose geometry is added from
    coordinates with `points()`, `line()` or `ring(coords, exterior)` (rings are rewound to the MVT
    winding as needed), without building a `geo_types` geometry.
  - New: `MvtFeatureBuilder::tag_ref(&str, MvtValueRef)` writes a borrowed tag, allocating only for a
    key or value the layer has not seen yet.
  - `MvtValueRef` no longer needs the `reader` feature, and gains `From<&MvtValue>`.
  - **Breaking:** `MvtTileBuilder::with_capacity`, `MvtTileBuilder::layer_with_capacity` and
    `MvtLayerBuilder::with_capacity` are removed, as the encoded size is not known up front. Use
    `MvtTileBuilder::new()`, `tile.layer(name)` and `MvtLayerBuilder::new(name)` instead.
  - The `writer` feature now uses `foldhash` instead of `dup-indexer`.

### Other

- fix release-plz CI and fmt
- reserve space for polygon ring closure ([#47](https://github.com/nyurik/fast-mvt/pull/47))
- detect code dups ([#51](https://github.com/nyurik/fast-mvt/pull/51))
- *(deps)* bump the all-cargo-version-updates group across 1 directory with 3 updates ([#44](https://github.com/nyurik/fast-mvt/pull/44))
- [pre-commit.ci] pre-commit autoupdate ([#41](https://github.com/nyurik/fast-mvt/pull/41))
- gungraun benchmarks ([#40](https://github.com/nyurik/fast-mvt/pull/40))

## [0.6.2](https://github.com/nyurik/fast-mvt/compare/v0.6.1...v0.6.2) - 2026-07-20

### Other

- some helper  functions, test cleanup, docs ([#32](https://github.com/nyurik/fast-mvt/pull/32))

## [0.6.1](https://github.com/nyurik/fast-mvt/compare/v0.6.0...v0.6.1) - 2026-07-18

### Added

- add tag_auto_int to pick most compact int store ([#30](https://github.com/nyurik/fast-mvt/pull/30))

### Other

- simplify parallel usage, add docs ([#31](https://github.com/nyurik/fast-mvt/pull/31))
- update readme

## [0.6.0](https://github.com/nyurik/fast-mvt/compare/v0.5.0...v0.6.0) - 2026-07-18

### Other

- [**breaking**] update to new buffa, some API changes ([#22](https://github.com/nyurik/fast-mvt/pull/22))

## [0.5.0](https://github.com/nyurik/fast-mvt/compare/v0.4.1...v0.5.0) - 2026-07-12

### Other

- debug format support, cleaner api ([#26](https://github.com/nyurik/fast-mvt/pull/26))
- minor internal cleanup ([#23](https://github.com/nyurik/fast-mvt/pull/23))

## [0.4.1](https://github.com/nyurik/fast-mvt/compare/v0.4.0...v0.4.1) - 2026-06-23

### Other

- update Cargo.lock dependencies

## [0.4.0](https://github.com/nyurik/fast-mvt/compare/v0.3.2...v0.4.0) - 2026-06-18

### Other

- [**breaking**] use ref geometry, add `encode_ref` support, faster indexer ([#16](https://github.com/nyurik/fast-mvt/pull/16))

## [0.3.2](https://github.com/nyurik/fast-mvt/compare/v0.3.1...v0.3.2) - 2026-06-17

### Other

- CI release workflow
- *(deps)* bump codecov/codecov-action ([#14](https://github.com/nyurik/fast-mvt/pull/14))
- ignore generated files in codecov

## [0.3.1](https://github.com/nyurik/fast-mvt/compare/v0.3.0...v0.3.1) - 2026-06-12

### Other

- add more tests ([#12](https://github.com/nyurik/fast-mvt/pull/12))
- use new buffa with 20% faster decoding ([#10](https://github.com/nyurik/fast-mvt/pull/10))

## [0.3.0](https://github.com/nyurik/fast-mvt/compare/v0.2.0...v0.3.0) - 2026-06-06

### Other

- [**breaking**] validate non-empty layer name, fuzz ([#8](https://github.com/nyurik/fast-mvt/pull/8))

## [0.2.0](https://github.com/nyurik/fast-mvt/compare/v0.1.2...v0.2.0) - 2026-06-06

### Breaking Changes

- MvtFeatureBuilder::id now takes option ([#6](https://github.com/nyurik/fast-mvt/pull/6))

## [0.1.2](https://github.com/nyurik/fast-mvt/compare/v0.1.1...v0.1.2) - 2026-06-06

### New

- json - mvt value conversion ([#4](https://github.com/nyurik/fast-mvt/pull/4))

## [0.1.1](https://github.com/nyurik/fast-mvt/compare/v0.1.0...v0.1.1) - 2026-06-06

- update readme
- format Cargo.toml, readme fixes ([#2](https://github.com/nyurik/fast-mvt/pull/2))

## 0.1.0 - 2026-06-06

- initial release
