use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{self, Read},
    ops::ControlFlow,
    path::{Path, PathBuf},
    process::Command,
};

pub struct Source {
    pub directory: &'static str,
    pub archive: &'static str,
    pub urls: &'static [&'static str],
    pub sha256: &'static str,
}

pub const SOURCES: &[Source] = &[
    Source {
        directory: "libinput-1.29.2",
        archive: "libinput-1.29.2.tar.gz",
        urls: &[
            "https://ftp2.osuosl.org/pub/blfs/conglomeration/Xorg/libinput-1.29.2.tar.gz",
            "https://mirror.freedif.org/pub/blfs/conglomeration/Xorg/libinput-1.29.2.tar.gz",
            "https://gitlab.freedesktop.org/libinput/libinput/-/archive/1.29.2/libinput-1.29.2.tar.gz",
        ],
        sha256: "02631a861fc6b9fb9ca6ae51cf863334e0699bf60c91c389ac420f3fa0053bdc",
    },
    Source {
        directory: "libevdev-1.13.4",
        archive: "libevdev-1.13.4.tar.xz",
        urls: &[
            "https://sources.voidlinux.org/libevdev-1.13.4/libevdev-1.13.4.tar.xz",
            "https://ftp2.osuosl.org/pub/blfs/conglomeration/Xorg/libevdev-1.13.4.tar.xz",
            "https://mirror.freedif.org/pub/blfs/conglomeration/Xorg/libevdev-1.13.4.tar.xz",
            "https://www.freedesktop.org/software/libevdev/libevdev-1.13.4.tar.xz",
        ],
        sha256: "f00ab8d42ad8b905296fab67e13b871f1a424839331516642100f82ad88127cd",
    },
    Source {
        directory: "mtdev-1.1.7",
        archive: "mtdev-1.1.7.tar.bz2",
        urls: &[
            "https://sources.voidlinux.org/mtdev-1.1.7/mtdev-1.1.7.tar.bz2",
            "https://ftp2.osuosl.org/pub/blfs/conglomeration/mtdev/mtdev-1.1.7.tar.bz2",
            "https://bitmath.org/code/mtdev/mtdev-1.1.7.tar.bz2",
        ],
        sha256: "a107adad2101fecac54ac7f9f0e0a0dd155d954193da55c2340c97f2ff1d814e",
    },
    Source {
        directory: "libudev-zero-1.0.3",
        archive: "libudev-zero-1.0.3.tar.gz",
        urls: &[
            "https://sources.buildroot.net/libudev-zero/libudev-zero-1.0.3.tar.gz",
            "https://dev.gateworks.com/sources/libudev-zero-1.0.3.tar.gz",
            "https://codeload.github.com/illiliti/libudev-zero/tar.gz/refs/tags/1.0.3",
        ],
        sha256: "0bd89b657d62d019598e6c7ed726ff8fed80e8ba092a83b484d66afb80b77da5",
    },
];

pub fn cache_dir(out: &Path, configured: Option<PathBuf>) -> PathBuf {
    // Cargo guarantees OUT_DIR is writable, even when the checkout is mounted
    // read-only or an external CARGO_TARGET_DIR is used.
    configured.unwrap_or_else(|| out.join("input-sources"))
}

fn error(message: impl Into<String>) -> io::Error {
    io::Error::other(message.into())
}

pub fn verify(path: &Path, expected: &str) -> io::Result<()> {
    let mut file = fs::File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = [0_u8; 65536];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
    }
    let actual = format!("{:x}", hash.finalize());
    if actual != expected {
        return Err(error(format!(
            "SHA-256 mismatch for {}: expected {expected}, got {actual}; remove the corrupt archive and rebuild",
            path.display()
        )));
    }
    Ok(())
}

fn download(url: &str, path: &Path) -> io::Result<()> {
    eprintln!("Downloading pinned input source {url}");
    let status = Command::new("curl")
        .args([
            "--fail",
            "--location",
            "--silent",
            "--show-error",
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
            "--connect-timeout",
            "15",
            "--max-time",
            "300",
            "--retry",
            "2",
            "--output",
        ])
        .arg(path)
        .arg(url)
        .status()
        .map_err(|e| error(format!("input source download requires curl: {e}")))?;
    if status.success() {
        Ok(())
    } else {
        Err(error(format!("download failed: {url} ({status})")))
    }
}

pub fn archive(source: &Source, cache: &Path, offline: bool) -> io::Result<PathBuf> {
    archive_with_download(source, cache, offline, download)
}

pub fn archive_with_download(
    source: &Source,
    cache: &Path,
    offline: bool,
    download: impl Fn(&str, &Path) -> io::Result<()>,
) -> io::Result<PathBuf> {
    let path = cache.join(source.archive);
    if path.exists() {
        verify(&path, source.sha256)?;
        return Ok(path);
    }
    if offline {
        return Err(error(format!(
            "offline input source cache is missing {}; pre-populate it from {} (SHA-256 {}) or rebuild without EMERGE_INPUT_OFFLINE/CARGO_NET_OFFLINE",
            path.display(),
            source.urls.join(", "),
            source.sha256
        )));
    }
    fs::create_dir_all(cache)?;
    // Separate Cargo builds may share a cache. Publish only complete, verified
    // archives, with a process-local temporary file and atomic rename.
    let partial = cache.join(format!(
        "{}.download-{}",
        source.archive,
        std::process::id()
    ));
    let result = (|| {
        let attempts = source.urls.iter().try_fold(
            error("no input source URLs configured"),
            |_previous, url| match download(url, &partial)
                .and_then(|()| verify(&partial, source.sha256))
            {
                Ok(()) => ControlFlow::Break(()),
                Err(failure) => {
                    eprintln!("Input source mirror {url} failed: {failure}");
                    let _ = fs::remove_file(&partial);
                    ControlFlow::Continue(failure)
                }
            },
        );
        if let ControlFlow::Continue(failure) = attempts {
            return Err(error(format!(
                "all input source mirrors failed for {} ({}): {failure}",
                source.archive,
                source.urls.join(", ")
            )));
        }
        fs::rename(&partial, &path)?;
        Ok(path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&partial);
    }
    result
}

pub fn unpack(source: &Source, cache: &Path, out: &Path, offline: bool) -> io::Result<PathBuf> {
    let archive = archive(source, cache, offline)?;
    let directory = out.join(source.directory);
    // Re-extract verified originals on each build-script invocation; patches
    // and generated files live outside these directories.
    if directory.exists() {
        fs::remove_dir_all(&directory)?;
    }
    fs::create_dir_all(out)?;
    let status = Command::new("tar")
        .arg("-xf")
        .arg(archive)
        .arg("-C")
        .arg(out)
        .status()
        .map_err(|e| {
            error(format!(
                "input source extraction requires tar with gzip/xz/bzip2 support: {e}"
            ))
        })?;
    if !status.success() || !directory.is_dir() {
        return Err(error(format!(
            "failed to extract {} ({status})",
            source.archive
        )));
    }
    Ok(directory)
}
