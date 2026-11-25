//! Pin directory capabilities at configuration. Re-open descendants one
//! component at a time without following symlinks, including after resolving
//! an explicitly permitted symlink. Check the opened descriptor, not a stat
//! followed by a separate, unconstrained read.
use super::*;
use std::path::PathBuf;

pub(crate) fn valid_absolute(name: &str) -> bool {
    name.strip_prefix('/')
        .is_some_and(|s| normal(s) && name.len() <= 4096)
}
pub(crate) fn valid_logical(name: &str) -> bool {
    name.len() <= 4096 && normal(name)
}
fn normal(name: &str) -> bool {
    !name.contains(['\0', '\\']) && name.split('/').all(|s| !matches!(s, "" | "." | ".."))
}
fn descend(root: &File, name: &str) -> Option<File> {
    if !normal(name) {
        return None;
    }
    let mut dir = root.try_clone().ok()?;
    let mut parts = name.split('/').peekable();
    while let Some(part) = parts.next() {
        let flags = OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK;
        dir = openat(
            dir,
            part,
            if parts.peek().is_some() {
                flags | OFlags::DIRECTORY
            } else {
                flags
            },
            Mode::empty(),
        )
        .ok()?
        .into();
    }
    Some(dir)
}
struct Allowed {
    name: PathBuf,
    fd: File,
}
pub(super) struct Files {
    root: Option<File>,
    allowed: Vec<Allowed>,
}
impl Files {
    pub fn new(settings: &Settings) -> Result<Self, String> {
        let [root] = settings.sources.as_slice() else {
            return Err("expected one application priv directory".into());
        };
        if !Path::new(root).is_absolute() {
            return Err("expected an absolute priv directory".into());
        }
        let dir = |p: &str| {
            rustix::fs::open(
                p,
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
                Mode::empty(),
            )
            .ok()
            .map(File::from)
        };
        // Mix may itself symlink the configured application priv directory.
        let root = dir(root);
        let slash = dir("/");
        let allowed = settings
            .allowlist
            .iter()
            .filter(|_| settings.runtime_enabled)
            .filter_map(|entry| {
                let name = if settings.follow_symlinks {
                    std::fs::canonicalize(entry).ok()?
                } else {
                    PathBuf::from(entry)
                };
                let relative = name.strip_prefix("/").ok()?.to_str()?;
                let fd = if relative.is_empty() {
                    slash.as_ref()?.try_clone().ok()?
                } else {
                    descend(slash.as_ref()?, relative)?
                };
                Some(Allowed { name, fd })
            })
            .collect();
        Ok(Self { root, allowed })
    }
    pub fn read(&self, name: &str, runtime: bool, settings: &Settings) -> Option<(Format, Data)> {
        let (file, format, limit) = if runtime {
            if !valid_absolute(name) || !settings.runtime_enabled {
                return None;
            }
            let format = Format::extension(name)?;
            let extension = Path::new(name).extension()?.to_str()?;
            if !settings.extensions.iter().any(|ext| {
                ext.strip_prefix('.')
                    .is_some_and(|s| s.eq_ignore_ascii_case(extension))
            }) {
                return None;
            }
            let resolved = if settings.follow_symlinks {
                std::fs::canonicalize(name).ok()?
            } else {
                PathBuf::from(name)
            };
            let file = self.allowed.iter().find_map(|root| {
                let rel = resolved.strip_prefix(&root.name).ok()?.to_str()?;
                if rel.is_empty() {
                    // A file allowlist entry pins an inode. Open a fresh descriptor
                    // without sharing its seek offset or trusting a replacement.
                    let slash: File = rustix::fs::open(
                        "/",
                        OFlags::RDONLY | OFlags::CLOEXEC | OFlags::DIRECTORY,
                        Mode::empty(),
                    )
                    .ok()?
                    .into();
                    let f = descend(&slash, resolved.strip_prefix("/").ok()?.to_str()?)?;
                    use std::os::unix::fs::MetadataExt;
                    let a = root.fd.metadata().ok()?;
                    let b = f.metadata().ok()?;
                    (a.dev() == b.dev() && a.ino() == b.ino()).then_some(f)
                } else {
                    descend(&root.fd, rel)
                }
            })?;
            (file, format, settings.max_file_size)
        } else {
            (
                descend(self.root.as_ref()?, name)?,
                Format::logical(name)?,
                MAX_FILE,
            )
        };
        let meta = file.metadata().ok()?;
        let len = usize::try_from(meta.len()).ok()?;
        if !meta.is_file() || !(1..=limit.min(format.limit())).contains(&len) {
            return None;
        }
        let data = Data::from_stream(file.take(len as u64), len)?;
        // An extension selects a decoder, even when another authorized input
        // with the very same bytes already populated a content-keyed cache.
        (Format::inline(&data) == format).then_some((format, data))
    }
}
