use emerge_skia::tree::deserialize::decode_tree;
use emerge_skia::tree::patch::decode_patches;
use std::path::{Path, PathBuf};

#[test]
fn benchmark_fixtures_decode() {
    let bench_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("bench");
    let fixture_roots = [
        bench_root.join("fixtures"),
        bench_root.join("external_fixtures"),
    ];

    let emrg_files: Vec<_> = fixture_roots
        .iter()
        .flat_map(|root| fixture_files(root, "emrg"))
        .collect();
    assert!(!emrg_files.is_empty(), "expected benchmark .emrg fixtures");
    for path in emrg_files {
        let bytes = std::fs::read(&path)
            .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()));
        decode_tree(&bytes).unwrap_or_else(|err| {
            panic!(
                "benchmark fixture {} does not decode: {err}",
                path.display()
            )
        });
    }

    for path in fixture_roots
        .iter()
        .flat_map(|root| fixture_files(root, "patch"))
    {
        let bytes = std::fs::read(&path)
            .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()));
        decode_patches(&bytes).unwrap_or_else(|err| {
            panic!(
                "benchmark patch fixture {} does not decode: {err}",
                path.display()
            )
        });
    }
}

fn fixture_files(root: &Path, extension: &str) -> Vec<PathBuf> {
    let mut files: Vec<_> = std::fs::read_dir(root)
        .unwrap_or_else(|err| {
            panic!(
                "failed to read benchmark fixture root {}: {err}",
                root.display()
            )
        })
        .map(|entry| {
            entry.unwrap_or_else(|err| {
                panic!(
                    "failed to read benchmark fixture entry in {}: {err}",
                    root.display()
                )
            })
        })
        .flat_map(|entry| {
            let path = entry.path();
            if path.is_dir() {
                fixture_files(&path, extension)
            } else if path.extension().is_some_and(|ext| ext == extension) {
                vec![path]
            } else {
                Vec::new()
            }
        })
        .collect();

    files.sort();
    files
}
