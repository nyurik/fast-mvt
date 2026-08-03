use std::fs;
use std::path::{Path, PathBuf};

// Harness-agnostic benchmark logic, shared verbatim by the criterion and gungraun bench binaries.
pub mod decode;
pub mod encode;

type MvtPath = (u64, PathBuf);

#[allow(dead_code)]
pub struct BenchTile {
    pub bytes: usize,
    pub data: Vec<u8>,
    pub parsed: fast_mvt::MvtTile,
}

#[must_use]
pub fn load_repo_mvt_files(allow_large: bool) -> Vec<BenchTile> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/mvt-fixtures/real-world");
    let mut paths = Vec::new();
    collect_mvt_paths(&dir, &mut paths);

    let paths = sample_mvt_paths(paths, allow_large);
    assert!(
        !paths.is_empty(),
        "no .mvt fixtures found in {}",
        dir.display()
    );
    let tiles = paths
        .into_iter()
        .map(|path| {
            let data = fs::read(&path)
                .unwrap_or_else(|err| panic!("can't read {}: {err}", path.display()));
            let bytes = data.len();
            (bytes, data)
        })
        // All benchmarked decoders must be able to traverse the same tile set.
        // Filter out fixtures that mvt-reader cannot inspect up front (both
        // harnesses load tiles before measuring), so its benchmark cannot panic
        // mid-measurement.
        .filter(|(_, data)| {
            mvt_reader::Reader::new(data.clone())
                .and_then(|reader| reader.get_layer_metadata().map(|_| ()))
                .is_ok()
        })
        .map(|(bytes, data)| {
            let parsed = fast_mvt::MvtReaderRef::new(&data)
                .and_then(|reader| reader.to_tile())
                .expect("decode fixture");
            BenchTile {
                bytes,
                data,
                parsed,
            }
        })
        .collect::<Vec<_>>();
    assert!(
        !tiles.is_empty(),
        "no mvt-reader-compatible .mvt fixtures found in {}",
        dir.display()
    );
    tiles
}

fn sample_mvt_paths(mut entries: Vec<MvtPath>, allow_large: bool) -> Vec<PathBuf> {
    const SMALLEST_COUNT: usize = 10;
    const MIDDLE_COUNT: usize = 20;
    const LARGEST_COUNT: usize = 3;
    let large_count = if allow_large { LARGEST_COUNT } else { 0 };

    entries.sort_by(|(left_bytes, left_path), (right_bytes, right_path)| {
        left_bytes
            .cmp(right_bytes)
            .then_with(|| left_path.cmp(right_path))
    });

    let len = entries.len();
    if len <= SMALLEST_COUNT + MIDDLE_COUNT + large_count {
        return entries.into_iter().map(|(_, path)| path).collect();
    }

    let mut indices = Vec::new();
    indices.extend(0..SMALLEST_COUNT);

    let centered_middle_start = (len - MIDDLE_COUNT) / 2;
    let middle_start =
        centered_middle_start.clamp(SMALLEST_COUNT, len - large_count - MIDDLE_COUNT);
    indices.extend(middle_start..middle_start + MIDDLE_COUNT);

    indices.extend(len - large_count..len);

    indices.sort_unstable();
    indices.dedup();

    indices.into_iter().map(|i| entries[i].1.clone()).collect()
}

fn collect_mvt_paths(dir: &Path, out: &mut Vec<MvtPath>) {
    let entries =
        fs::read_dir(dir).unwrap_or_else(|err| panic!("can't read {}: {err}", dir.display()));
    for entry in entries {
        let path = entry
            .unwrap_or_else(|err| panic!("can't read entry in {}: {err}", dir.display()))
            .path();
        if path.is_dir() {
            collect_mvt_paths(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "mvt") {
            let bytes = fs::metadata(&path)
                .unwrap_or_else(|err| panic!("can't read metadata for {}: {err}", path.display()))
                .len();
            out.push((bytes, path));
        }
    }
}
