use super::{Attribute, Attributes, Candidate, DeviceId};
use std::{
    collections::{HashMap, HashSet},
    ffi::CStr,
    fs::{File, OpenOptions},
    io::{self, Read, Seek, SeekFrom, Write},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::fs::{MetadataExt, OpenOptionsExt},
    },
    path::{Path, PathBuf},
    sync::Arc,
};

pub(super) fn identity(path: &Path) -> Result<DeviceId, String> {
    let metadata = path
        .metadata()
        .map_err(|e| format!("{}: {e}", path.display()))?;
    directory_id(&metadata)
}

fn directory_id(metadata: &std::fs::Metadata) -> Result<DeviceId, String> {
    if !metadata.is_dir() {
        return Err("backlight device is not a directory".into());
    }
    Ok(DeviceId {
        device: metadata.dev(),
        inode: metadata.ino(),
    })
}

/// Read-only discovery. Only connector ancestry is positive evidence; a device
/// link to a shared GPU is not enough. Missing class support yields no candidates,
/// but a broken/changing inventory or permission error fails rather than guessing.
pub fn discover(class_root: &Path, connectors: &[PathBuf]) -> Result<Vec<Candidate>, String> {
    let entries = match std::fs::read_dir(class_root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.to_string()),
    };
    let connectors = connectors
        .iter()
        .map(|path| path.canonicalize().map_err(|e| e.to_string()))
        .collect::<Result<HashSet<_>, _>>()?;
    let candidates = entries
        .map(|entry| {
            let path = entry
                .map_err(|e| e.to_string())?
                .path()
                .canonicalize()
                .map_err(|e| e.to_string())?;
            let directory = Arc::new(
                OpenOptions::new()
                    .read(true)
                    .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
                    .open(&path)
                    .map_err(|e| e.to_string())?,
            );
            let identity = directory_id(&directory.metadata().map_err(|e| e.to_string())?)?;
            let device = match path.join("device").canonicalize() {
                Ok(path) => Some(path),
                Err(error) if error.kind() == io::ErrorKind::NotFound => None,
                Err(error) => return Err(error.to_string()),
            };
            let mut matches = connectors.iter().filter(|connector| {
                path.starts_with(connector)
                    || device
                        .as_ref()
                        .is_some_and(|device| device.starts_with(connector))
            });
            let connector = matches.next().cloned();
            if matches.next().is_some() {
                return Err("backlight has ambiguous connector ancestry".into());
            }
            Ok((
                identity,
                Candidate {
                    path,
                    identity,
                    connector,
                    directory,
                },
            ))
        })
        .collect::<Result<HashMap<_, _>, String>>()?;
    let mut candidates = candidates.into_values().collect::<Vec<_>>();
    candidates.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(candidates)
}

/// File descriptors are anchored to the selected directory, not repeatedly
/// opened through mutable class symlinks. Ordinary files in private test trees
/// exercise this path; numerical sysfs replacement semantics use fake Attributes.
pub struct SysfsAttributes {
    directory: Arc<File>,
    maximum: File,
    brightness: File,
    power: File,
}

impl SysfsAttributes {
    pub fn open(candidate: &Candidate) -> Result<Self, String> {
        let directory = Arc::clone(&candidate.directory);
        if directory_id(&directory.metadata().map_err(|e| e.to_string())?)? != candidate.identity {
            return Err("backlight changed after discovery".into());
        }
        let open = |name: &CStr, flags| {
            open_at(&directory, name, flags).map_err(|error| {
                format!(
                    "backlight {} / {}: {error}",
                    candidate.path.display(),
                    name.to_string_lossy()
                )
            })
        };
        let maximum = open(c"max_brightness", libc::O_RDONLY)?;
        let brightness = open(c"brightness", libc::O_RDWR)?;
        let power = open(c"bl_power", libc::O_RDWR)?;
        Ok(Self {
            directory,
            maximum,
            brightness,
            power,
        })
    }
}

fn open_at(directory: &File, name: &CStr, flags: i32) -> io::Result<File> {
    // SAFETY: name is a static NUL-terminated attribute, directory is kept alive,
    // and no creation flags are used. Symlink attributes are explicitly rejected.
    let fd = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            name.as_ptr(),
            flags | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: successful openat returned a fresh, uniquely owned descriptor.
    Ok(unsafe { File::from_raw_fd(fd) })
}

fn read_number(file: &mut File) -> Result<u32, String> {
    file.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
    let mut text = String::new();
    file.take(65)
        .read_to_string(&mut text)
        .map_err(|e| e.to_string())?;
    if text.len() > 64 {
        return Err("oversized backlight attribute".into());
    }
    text.trim()
        .parse()
        .map_err(|_| "invalid unsigned backlight attribute".into())
}

impl Attributes for SysfsAttributes {
    fn read(&mut self, attribute: Attribute) -> Result<Option<u32>, String> {
        match attribute {
            Attribute::Maximum => read_number(&mut self.maximum).map(Some),
            Attribute::Brightness => read_number(&mut self.brightness).map(Some),
            Attribute::Power => read_number(&mut self.power).map(Some),
            Attribute::Actual => {
                match open_at(&self.directory, c"actual_brightness", libc::O_RDONLY) {
                    Ok(mut file) => read_number(&mut file).map(Some),
                    Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
                    Err(error) => Err(error.to_string()),
                }
            }
        }
    }

    fn write(&mut self, attribute: Attribute, value: u32) -> Result<(), String> {
        let file = match attribute {
            Attribute::Brightness => &mut self.brightness,
            Attribute::Power => &mut self.power,
            _ => return Err("backlight attribute is read-only".into()),
        };
        let text = format!("{value}\n");
        file.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
        let count = file.write(text.as_bytes()).map_err(|e| e.to_string())?;
        if count != text.len() {
            return Err("short backlight write; hardware state is uncertain".into());
        }
        Ok(())
    }
}
