use std::fs;
use std::path::{Path, PathBuf};

type MvtPath = (u64, PathBuf);

#[must_use]
pub fn load_repo_mvt_files() -> Vec<Vec<u8>> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/mvt-fixtures/real-world");
    let mut paths = Vec::new();
    collect_mvt_paths(&dir, &mut paths);

    let paths = sample_mvt_paths(paths);
    assert!(
        !paths.is_empty(),
        "no .mvt fixtures found in {}",
        dir.display()
    );
    paths
        .into_iter()
        .map(|path| {
            fs::read(&path).unwrap_or_else(|err| panic!("can't read {}: {err}", path.display()))
        })
        .collect()
}

fn sample_mvt_paths(mut entries: Vec<MvtPath>) -> Vec<PathBuf> {
    const SMALLEST_COUNT: usize = 20;
    const MIDDLE_COUNT: usize = 5;
    const LARGEST_COUNT: usize = 1;

    entries.sort_by(|(left_bytes, left_path), (right_bytes, right_path)| {
        left_bytes
            .cmp(right_bytes)
            .then_with(|| left_path.cmp(right_path))
    });

    let len = entries.len();
    if len <= SMALLEST_COUNT + MIDDLE_COUNT + LARGEST_COUNT {
        return entries.into_iter().map(|(_, path)| path).collect();
    }

    let mut indices = Vec::new();
    indices.extend(0..SMALLEST_COUNT);

    let centered_middle_start = (len - MIDDLE_COUNT) / 2;
    let middle_start =
        centered_middle_start.clamp(SMALLEST_COUNT, len - LARGEST_COUNT - MIDDLE_COUNT);
    indices.extend(middle_start..middle_start + MIDDLE_COUNT);

    indices.extend(len - LARGEST_COUNT..len);

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
