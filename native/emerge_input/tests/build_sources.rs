#[path = "../event_names.rs"]
mod event_names;
#[path = "../sources.rs"]
mod sources;

use sha2::{Digest, Sha256};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

struct TempDir(PathBuf);
impl TempDir {
    fn new() -> Self {
        static ID: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "emerge-input-source-test-{}-{}",
            std::process::id(),
            ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

const FIXTURE: sources::Source = sources::Source {
    directory: "fixture",
    archive: "fixture.tar.gz",
    url: "https://invalid.example/fixture.tar.gz",
    sha256: "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
};

#[test]
fn cached_archives_are_verified_even_when_offline() {
    let cache = TempDir::new();
    let path = cache.0.join(FIXTURE.archive);
    fs::write(&path, b"abc").unwrap();
    assert_eq!(sources::archive(&FIXTURE, &cache.0, true).unwrap(), path);
    assert_eq!(sources::archive(&FIXTURE, &cache.0, false).unwrap(), path);
    fs::write(&path, b"corrupted").unwrap();
    for offline in [true, false] {
        let err = sources::archive(&FIXTURE, &cache.0, offline).unwrap_err();
        assert!(err.to_string().contains("SHA-256 mismatch"));
    }
}

#[test]
fn missing_offline_archive_fails_without_creating_or_extracting_files() {
    let cache = TempDir::new();
    let output = cache.0.join("output");
    let err = sources::unpack(&FIXTURE, &cache.0, &output, true).unwrap_err();
    assert!(
        err.to_string()
            .contains("offline input source cache is missing")
    );
    assert!(err.to_string().contains(FIXTURE.url));
    assert!(!output.exists());
    assert_eq!(fs::read_dir(&cache.0).unwrap().count(), 0);
}

#[test]
fn corrupt_archive_is_rejected_before_extraction() {
    let cache = TempDir::new();
    fs::write(cache.0.join(FIXTURE.archive), b"not a tar file").unwrap();
    let output = cache.0.join("output");
    let err = sources::unpack(&FIXTURE, &cache.0, &output, true).unwrap_err();
    assert!(err.to_string().contains("SHA-256 mismatch"));
    assert!(!output.exists());
}

#[test]
fn sources_are_immutable_https_archives_with_sha256_pins() {
    assert_eq!(sources::SOURCES.len(), 4);
    for source in sources::SOURCES {
        assert!(source.url.starts_with("https://"));
        assert!(source.archive.starts_with(source.directory));
        assert_eq!(source.sha256.len(), 64);
        assert!(source.sha256.chars().all(|c| c.is_ascii_hexdigit()));
        assert!(!source.url.contains("latest"));
    }
}

#[test]
fn event_names_match_the_pinned_upstream_generator_output() {
    let generated = include_bytes!(concat!(env!("OUT_DIR"), "/event-names.h"));
    assert_eq!(
        format!("{:x}", Sha256::digest(generated)),
        "bb9681f0e54f3517cad6803e2d0b193e6cf9ab7c06e73d75f485f237304d90e3"
    );
}

#[test]
fn event_names_keep_numeric_canonical_names_and_sorted_alias_lookups() {
    let generated = event_names::generate(
        "#define EV_SYN 0\n#define EV_KEY 1\n#define EV_MAX 2\n\
         #define KEY_A 30\n#define KEY_B 31\n#define BTN_LEFT 0x110\n\
         #define BTN_MOUSE 0x110\n#define BTN_A BTN_SOUTH\n\
         #define SW_LID 0\n#define SW_MAX 0\n#define REP_DELAY 0\n#define REP_MAX 0\n",
    );
    assert!(generated.contains("[KEY_A] = \"KEY_A\""));
    assert!(generated.contains("[BTN_LEFT] = \"BTN_LEFT\""));
    assert!(!generated.contains("[BTN_MOUSE]"));
    assert!(!generated.contains("[SW_MAX]"));
    assert!(generated.contains("{ .name = \"SW_MAX\", .value = SW_MAX }"));
    assert!(generated.contains("{ .name = \"BTN_A\", .value = BTN_A }"));
    assert!(generated.find(".name = \"BTN_A\"") < generated.find(".name = \"BTN_LEFT\""));
    assert!(generated.contains("    SYN_MAX,\n    KEY_MAX,\n    -1,"));
}
