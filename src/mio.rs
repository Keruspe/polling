//! Bindings to epoll (Linux, Android).

use std::convert::TryInto;
use std::io;
use std::os::unix::io::{AsRawFd, RawFd};
use std::sync::Mutex;
use std::time::Duration;

use crate::timerfd::TimerFd;
use crate::Event;

use mio::{unix::SourceFd, Interest, Poll, Registry, Token, Waker};

/// Interface to epoll.
#[derive(Debug)]
pub struct Poller {
    poll: Mutex<Poll>,
    registry: Registry,
    waker: Waker,
    /// File descriptor for the timerfd that produces timeouts.
    timer_fd: Option<TimerFd>,
}

impl From<Event> for Interest {
    fn from(event: Event) -> Self {
        if event.writable {
            if event.readable {
                Interest::READABLE.add(Interest::WRITABLE)
            } else {
                Interest::WRITABLE
            }
        } else {
            Interest::READABLE
        }
    }
}

impl Poller {
    /// Creates a new poller.
    pub fn new() -> io::Result<Poller> {
        let poll = Poll::new()?;
        let waker = Waker::new(poll.registry(), Token(crate::NOTIFY_KEY))?;
        let registry = poll.registry().try_clone()?;
        let poll = Mutex::new(poll);
        let timer_fd = TimerFd::new().ok();
        let poller = Poller {
            poll,
            registry,
            waker,
            timer_fd,
        };

        if let Some(timer_fd) = poller.timer_fd.as_ref() {
            poller.add(timer_fd.as_raw_fd(), Event::none(crate::NOTIFY_KEY))?;
        }

        log::trace!("new: mio");
        Ok(poller)
    }

    /// Adds a new file descriptor.
    pub fn add(&self, fd: RawFd, ev: Event) -> io::Result<()> {
        log::trace!("add: fd={}, ev={:?}", fd, ev);
        self.registry
            .register(&mut SourceFd(&fd), Token(ev.key), ev.into())
    }

    /// Modifies an existing file descriptor.
    pub fn modify(&self, fd: RawFd, ev: Event) -> io::Result<()> {
        log::trace!("modify: fd={}, ev={:?}", fd, ev);
        self.registry
            .reregister(&mut SourceFd(&fd), Token(ev.key), ev.into())
    }

    /// Deletes a file descriptor.
    pub fn delete(&self, fd: RawFd) -> io::Result<()> {
        log::trace!("remove: fd={}", fd);
        self.registry.deregister(&mut SourceFd(&fd))
    }

    /// Waits for I/O events with an optional timeout.
    pub fn wait(&self, events: &mut Events, mut timeout: Option<Duration>) -> io::Result<()> {
        log::trace!("wait: timeout={:?}", timeout);

        if let Some(timer_fd) = self.timer_fd.as_ref() {
            // Configure the timeout using timerfd.
            timer_fd.set_timeout(timeout)?;

            // Set interest in timerfd.
            self.modify(
                timer_fd.as_raw_fd(),
                Event {
                    key: crate::NOTIFY_KEY,
                    readable: true,
                    writable: false,
                },
            )?;
        }

        // Timeout in milliseconds for epoll.
        if self.timer_fd.is_none() {
            if let Some(t) = timeout {
                // Round up to a whole millisecond.
                timeout = Some(Duration::from_millis(
                    t.as_millis()
                        .try_into()
                        .unwrap_or(std::u64::MAX)
                        .saturating_add(1),
                ));
            }
        } else if let Some(t) = timeout {
            if t != Duration::ZERO {
                timeout = None;
            }
        }

        self.poll.lock().unwrap().poll(&mut events.inner, timeout)?;
        events.len = events.inner.iter().count() as usize;
        log::trace!("new events: len={}", events.len);

        Ok(())
    }

    /// Sends a notification to wake up the current or next `wait()` call.
    pub fn notify(&self) -> io::Result<()> {
        log::trace!("notify: mio");
        self.waker.wake()
    }
}

impl Drop for Poller {
    fn drop(&mut self) {
        if let Some(timer_fd) = self.timer_fd.as_ref() {
            let _ = self.delete(timer_fd.as_raw_fd());
        }
    }
}

/// A list of reported I/O events.
pub struct Events {
    inner: mio::Events,
    len: usize,
}

unsafe impl Send for Events {}

impl Events {
    /// Creates an empty list.
    pub fn new() -> Self {
        let inner = mio::Events::with_capacity(1024);
        let len = 0;
        Events { inner, len }
    }

    /// Iterates over I/O events.
    pub fn iter(&self) -> impl Iterator<Item = Event> + '_ {
        self.inner.iter().map(|ev| Event {
            key: ev.token().0,
            readable: ev.is_readable() || ev.is_read_closed() || ev.is_error() || ev.is_priority(),
            writable: ev.is_writable() || ev.is_write_closed() || ev.is_error(),
        })
    }
}
