//! A daemonless, statically bundled libinput path context.
//! The context and owning device references never leave their input thread.
use std::{
    collections::HashMap,
    ffi::{CString, c_void},
    io,
    marker::PhantomData,
    os::{
        fd::{AsRawFd, RawFd},
        unix::ffi::OsStrExt,
    },
    path::{Path, PathBuf},
    ptr::NonNull,
    rc::Rc,
};

#[derive(Default, Clone, Copy, Debug)]
#[repr(C)]
pub struct Event {
    pub device: u64,
    pub kind: u32,
    pub code: u32,
    pub state: u32,
    pub slot: i32,
    pub x: f64,
    pub y: f64,
}
unsafe extern "C" {
    fn emerge_input_create() -> *mut c_void;
    fn emerge_input_destroy(ctx: *mut c_void);
    fn emerge_input_logging(ctx: *mut c_void, enabled: i32);
    fn emerge_input_fd(ctx: *mut c_void) -> i32;
    fn emerge_input_dispatch(ctx: *mut c_void) -> i32;
    fn emerge_input_add(ctx: *mut c_void, path: *const libc::c_char) -> *mut c_void;
    fn emerge_input_remove(device: *mut c_void);
    fn emerge_input_next(ctx: *mut c_void, width: u32, height: u32, out: *mut Event) -> i32;
    fn emerge_input_validate_quirks(ctx: *mut c_void) -> i32;
}

pub struct Context {
    ctx: NonNull<c_void>,
    devices: HashMap<PathBuf, NonNull<c_void>>,
    _thread_local: PhantomData<Rc<()>>,
}
impl Context {
    pub fn new() -> io::Result<Self> {
        // SAFETY: the C constructor returns a new owning reference or NULL.
        let ctx =
            NonNull::new(unsafe { emerge_input_create() }).ok_or_else(io::Error::last_os_error)?;
        Ok(Self {
            ctx,
            devices: HashMap::new(),
            _thread_local: PhantomData,
        })
    }
    pub fn set_logging(&mut self, enabled: bool) {
        // SAFETY: the C-only logger is installed on this live, exclusive context.
        unsafe { emerge_input_logging(self.ctx.as_ptr(), i32::from(enabled)) };
    }
    pub fn add(&mut self, path: &Path) -> bool {
        if self.devices.contains_key(path) {
            return true;
        }
        let Ok(path_c) = CString::new(path.as_os_str().as_bytes()) else {
            return false;
        };
        // SAFETY: context is live and path_c is terminated for the synchronous call.
        let Some(device) =
            NonNull::new(unsafe { emerge_input_add(self.ctx.as_ptr(), path_c.as_ptr()) })
        else {
            return false;
        };
        self.devices.insert(path.to_path_buf(), device);
        true
    }
    pub fn remove(&mut self, path: &Path) {
        if let Some(device) = self.devices.remove(path) {
            // SAFETY: consume our owning device reference exactly once. The
            // shim also tolerates a device already detached by libinput.
            unsafe { emerge_input_remove(device.as_ptr()) };
        }
    }
    pub fn removed_device(&mut self, id: u64) -> Vec<PathBuf> {
        let paths: Vec<_> = self
            .devices
            .iter()
            .filter(|(_, ptr)| ptr.as_ptr() as usize as u64 == id)
            .map(|(path, _)| path.clone())
            .collect();
        for path in &paths {
            self.remove(path);
        }
        paths
    }
    pub fn dispatch(&mut self) -> io::Result<()> {
        // SAFETY: exclusive access to a live thread-local context.
        let rc = unsafe { emerge_input_dispatch(self.ctx.as_ptr()) };
        if rc < 0 {
            Err(io::Error::from_raw_os_error(-rc))
        } else {
            Ok(())
        }
    }
    pub fn next_event(&mut self, size: (u32, u32)) -> Option<Event> {
        let mut out = Event::default();
        // SAFETY: Event's repr(C) matches shim.c, which initializes every field
        // and destroys the native event before returning its owned scalar copy.
        (unsafe { emerge_input_next(self.ctx.as_ptr(), size.0.max(1), size.1.max(1), &mut out) }
            != 0)
            .then_some(out)
    }
    pub fn validate_bundled_quirks(&mut self) -> bool {
        // SAFETY: parsing uses immutable compiled-in data and a live context.
        unsafe { emerge_input_validate_quirks(self.ctx.as_ptr()) != 0 }
    }
}
impl AsRawFd for Context {
    fn as_raw_fd(&self) -> RawFd {
        // SAFETY: the FD remains owned by the live context.
        unsafe { emerge_input_fd(self.ctx.as_ptr()) }
    }
}
impl Drop for Context {
    fn drop(&mut self) {
        for (_, device) in self.devices.drain() {
            // SAFETY: release each additional owning device reference once.
            unsafe { emerge_input_remove(device.as_ptr()) };
        }
        // SAFETY: this is the one owning context reference.
        unsafe { emerge_input_destroy(self.ctx.as_ptr()) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    unsafe extern "C" {
        fn emerge_input_test_open(path: *const libc::c_char) -> i32;
        fn emerge_input_classify(
            ev: *const libc::c_char,
            abs: *const libc::c_char,
            rel: *const libc::c_char,
            keys: *const libc::c_char,
            props: *const libc::c_char,
        ) -> i32;
    }
    fn bits(codes: &[usize]) -> CString {
        let width = std::mem::size_of::<libc::c_ulong>() * 8;
        let n = codes.iter().max().map_or(1, |code| code / width + 1);
        let words: Vec<_> = (0..n)
            .rev()
            .map(|word| {
                let value = codes
                    .iter()
                    .filter(|code| **code / width == word)
                    .fold(0_u64, |value, code| value | (1 << (code % width)));
                format!("{value:x}")
            })
            .collect();
        CString::new(words.join(" ")).unwrap()
    }
    fn classify(
        ev: &[usize],
        abs: &[usize],
        rel: &[usize],
        keys: &[usize],
        props: &[usize],
    ) -> i32 {
        let [ev, abs, rel, keys, props] = [ev, abs, rel, keys, props].map(bits);
        // SAFETY: each CString remains valid for this synchronous test call.
        unsafe {
            emerge_input_classify(
                ev.as_ptr(),
                abs.as_ptr(),
                rel.as_ptr(),
                keys.as_ptr(),
                props.as_ptr(),
            )
        }
    }
    #[test]
    fn capabilities_classify_input_without_a_udev_database() {
        assert_eq!(classify(&[1, 3], &[53, 54], &[], &[330], &[1]), 4); // MT-only direct touchscreen
        assert_eq!(classify(&[3], &[53, 54], &[], &[], &[1]), 4); // no legacy BTN_TOUCH
        assert_eq!(classify(&[1, 3], &[0, 1], &[], &[272], &[]), 1); // absolute mouse
        assert_eq!(
            classify(&[1, 2, 3], &[53, 54], &[0, 1], &[272, 330], &[1]),
            1 | 4
        ); // composite REL/ABS
        assert_eq!(classify(&[1, 3], &[0, 1], &[], &[330, 325], &[1]), 4); // DIRECT overrides FINGER
        assert_eq!(classify(&[1, 3], &[0, 1], &[], &[330, 325], &[0]), 8); // touchpad
        assert_eq!(classify(&[1, 2], &[], &[0, 1], &[272], &[]), 1); // mouse
        assert_eq!(classify(&[1], &[], &[], &[28, 30], &[]), 2 | 256); // keyboard
        assert_eq!(classify(&[1, 3], &[0, 1], &[], &[304], &[]), 0); // not every ABS device is touch
        assert_eq!(classify(&[3], &[0, 1, 2], &[], &[], &[]), 32); // accelerometer
    }
    #[test]
    fn restricted_open_needs_only_read_access_and_owns_a_nonblocking_cloexec_fd() {
        use std::os::fd::{FromRawFd, OwnedFd};
        let path = CString::new("/dev/null").unwrap();
        // SAFETY: valid terminated path; the shim returns an owning FD or error.
        let fd = unsafe { emerge_input_test_open(path.as_ptr()) };
        assert!(fd >= 0);
        // SAFETY: the returned FD is exclusively owned; fcntl has no pointer args.
        let fd = unsafe { OwnedFd::from_raw_fd(fd) };
        let flags = unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_GETFL) };
        assert_eq!(flags & libc::O_ACCMODE, libc::O_RDONLY);
        assert_ne!(flags & libc::O_NONBLOCK, 0);
        let flags = unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_GETFD) };
        assert_ne!(flags & libc::FD_CLOEXEC, 0);
    }

    #[test]
    fn daemonless_context_and_embedded_quirks() {
        let mut ctx = Context::new().unwrap();
        assert!(ctx.as_raw_fd() >= 0);
        assert!(ctx.validate_bundled_quirks());
        assert!(!ctx.add(Path::new("/definitely/missing/input-device")));
        ctx.dispatch().unwrap();
        assert!(ctx.next_event((10, 10)).is_none());
    }
}
