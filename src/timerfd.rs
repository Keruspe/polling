//! Bindings to timer_fd

use std::io;
use std::os::unix::io::{AsRawFd, RawFd};
use std::ptr;
use std::time::Duration;

/// Interface to timer_fd.
#[derive(Debug)]
pub struct TimerFd {
    /// File descriptor for the inner timerfd.
    timer_fd: RawFd,
}

impl TimerFd {
    /// Creates a new timer_fd.
    pub fn new() -> io::Result<Self> {
        // Create an epoll instance.
        //
        // Use `epoll_create1` with `EPOLL_CLOEXEC`.
        let timer_fd = syscall!(syscall(
            libc::SYS_timerfd_create,
            libc::CLOCK_MONOTONIC as libc::c_int,
            (libc::TFD_CLOEXEC | libc::TFD_NONBLOCK) as libc::c_int,
        ))? as RawFd;

        Ok(TimerFd { timer_fd })
    }

    /// Set the timeout at which the timer_fs will fire an event.
    pub fn set_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        // Configure the timeout using timerfd.
        let new_val = libc::itimerspec {
            it_interval: TS_ZERO,
            it_value: match timeout {
                None => TS_ZERO,
                Some(t) => libc::timespec {
                    tv_sec: t.as_secs() as libc::time_t,
                    tv_nsec: (t.subsec_nanos() as libc::c_long).into(),
                },
            },
        };

        syscall!(syscall(
            libc::SYS_timerfd_settime,
            self.timer_fd as libc::c_int,
            0 as libc::c_int,
            &new_val as *const libc::itimerspec,
            ptr::null_mut() as *mut libc::itimerspec
        ))?;

        Ok(())
    }
}

impl AsRawFd for TimerFd {
    fn as_raw_fd(&self) -> RawFd {
        self.timer_fd
    }
}

impl Drop for TimerFd {
    fn drop(&mut self) {
        let _ = syscall!(close(self.timer_fd));
    }
}

/// `timespec` value that equals zero.
const TS_ZERO: libc::timespec = libc::timespec {
    tv_sec: 0,
    tv_nsec: 0,
};
