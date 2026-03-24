use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::sync::Arc;
use std::time::Duration;

#[derive(Clone, Debug)]
pub struct EventFd {
    fd: Arc<OwnedFd>,
}

impl EventFd {
    pub fn new() -> io::Result<Self> {
        let fd = unsafe { libc::eventfd(0, libc::EFD_CLOEXEC | libc::EFD_NONBLOCK) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }

        Ok(Self {
            fd: Arc::new(unsafe { OwnedFd::from_raw_fd(fd) }),
        })
    }

    pub fn signal(&self) -> io::Result<()> {
        let value = 1u64.to_ne_bytes();
        loop {
            let written = unsafe {
                libc::write(
                    self.as_raw_fd(),
                    value.as_ptr().cast::<libc::c_void>(),
                    value.len(),
                )
            };

            if written == value.len() as isize {
                return Ok(());
            }

            if written < 0 {
                let err = io::Error::last_os_error();
                if err.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(err);
            }

            return Err(io::Error::new(
                io::ErrorKind::WriteZero,
                "short write to eventfd",
            ));
        }
    }

    pub fn drain(&self) -> io::Result<u64> {
        let mut value = 0u64;
        loop {
            let read = unsafe {
                libc::read(
                    self.as_raw_fd(),
                    (&mut value as *mut u64).cast::<libc::c_void>(),
                    std::mem::size_of::<u64>(),
                )
            };

            if read == std::mem::size_of::<u64>() as isize {
                return Ok(value);
            }

            if read < 0 {
                let err = io::Error::last_os_error();
                if err.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(err);
            }

            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "short read from eventfd",
            ));
        }
    }

    pub fn as_raw_fd(&self) -> RawFd {
        self.fd.as_raw_fd()
    }
}

pub fn poll_fds(fds: &mut [libc::pollfd], timeout: Option<Duration>) -> io::Result<usize> {
    let timeout_ms = poll_timeout_ms(timeout);
    loop {
        let ready = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, timeout_ms) };
        if ready >= 0 {
            return Ok(ready as usize);
        }

        let err = io::Error::last_os_error();
        if err.kind() == io::ErrorKind::Interrupted {
            continue;
        }
        return Err(err);
    }
}

pub fn poll_timeout_ms(timeout: Option<Duration>) -> libc::c_int {
    match timeout {
        None => -1,
        Some(duration) => {
            let millis = duration.as_nanos().div_ceil(1_000_000);
            if millis == 0 {
                0
            } else {
                millis.min(libc::c_int::MAX as u128) as libc::c_int
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn poll_timeout_ms_maps_none_and_zero() {
        assert_eq!(poll_timeout_ms(None), -1);
        assert_eq!(poll_timeout_ms(Some(Duration::ZERO)), 0);
    }

    #[test]
    fn poll_timeout_ms_rounds_positive_durations_up() {
        assert_eq!(poll_timeout_ms(Some(Duration::from_nanos(1))), 1);
        assert_eq!(poll_timeout_ms(Some(Duration::from_micros(500))), 1);
        assert_eq!(poll_timeout_ms(Some(Duration::from_micros(1_500))), 2);
    }

    #[test]
    fn eventfd_signal_and_drain_round_trip() {
        let wake = EventFd::new().expect("eventfd available");
        wake.signal().expect("signal succeeds");
        assert_eq!(wake.drain().expect("drain succeeds"), 1);
    }
}
