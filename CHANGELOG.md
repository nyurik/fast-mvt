# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
