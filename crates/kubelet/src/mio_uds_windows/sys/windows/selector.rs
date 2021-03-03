use std::io;
use std::os::windows::prelude::*;
use std::sync::Mutex;

use crate::mio_uds_windows::poll;
use mio::windows::Binding;
use mio::{Evented, Poll, PollOpt, Ready, Registration, SetReadiness, Token};
use tracing::trace;

/// Helper struct used for TCP and UDP which bundles a `binding` with a
/// `SetReadiness` handle.
pub struct ReadyBinding {
    binding: Binding,
    readiness: Option<SetReadiness>,
}

impl ReadyBinding {
    /// Creates a new blank binding ready to be inserted into an I/O object.
    ///
    /// Won't actually do anything until associated with an `Selector` loop.
    pub fn new() -> ReadyBinding {
        ReadyBinding {
            binding: Binding::new(),
            readiness: None,
        }
    }

    /// Returns whether this binding has been associated with a selector
    /// yet.
    pub fn registered(&self) -> bool {
        self.readiness.is_some()
    }

    /// Acquires a buffer with at least `size` capacity.
    ///
    /// If associated with a selector, this will attempt to pull a buffer from
    /// that buffer pool. If not associated with a selector, this will allocate
    /// a fresh buffer.
    pub fn get_buffer(&self, size: usize) -> Vec<u8> {
        poll::skinny::get_buffer(&self.binding, size)
    }

    /// Returns a buffer to this binding.
    ///
    /// If associated with a selector, this will push the buffer back into the
    /// selector's pool of buffers. Otherwise this will just drop the buffer.
    pub fn put_buffer(&self, buf: Vec<u8>) {
        poll::skinny::put_buffer(&self.binding, buf)
    }

    /// Sets the readiness of this I/O object to a particular `set`.
    ///
    /// This is later used to fill out and respond to requests to `poll`. Note
    /// that this is all implemented through the `SetReadiness` structure in the
    /// `poll` module.
    pub fn set_readiness(&self, set: Ready) {
        if let Some(ref i) = self.readiness {
            trace!("set readiness to {:?}", set);
            i.set_readiness(set).expect("event loop disappeared?");
        }
    }

    /// Queries what the current readiness of this I/O object is.
    ///
    /// This is what's being used to generate events returned by `poll`.
    pub fn readiness(&self) -> Ready {
        match self.readiness {
            Some(ref i) => i.readiness(),
            None => Ready::empty(),
        }
    }

    /// Implementation of the `Evented::register` function essentially.
    ///
    /// Returns an error if we're already registered with another event loop,
    /// and otherwise just reassociates ourselves with the event loop to
    /// possible change tokens.
    pub fn register_socket(
        &mut self,
        socket: &dyn AsRawSocket,
        poll: &Poll,
        token: Token,
        events: Ready,
        opts: PollOpt,
        registration: &Mutex<Option<Registration>>,
    ) -> io::Result<()> {
        trace!("register {:?} {:?}", token, events);
        unsafe {
            self.binding.register_socket(socket, token, poll)?;
        }

        let (r, s) = poll::new_registration(poll, token, events, opts);
        self.readiness = Some(s);
        *registration.lock().unwrap() = Some(r);
        Ok(())
    }

    /// Implementation of `Evented::reregister` function.
    pub fn reregister_socket(
        &mut self,
        socket: &dyn AsRawSocket,
        poll: &Poll,
        token: Token,
        events: Ready,
        opts: PollOpt,
        registration: &Mutex<Option<Registration>>,
    ) -> io::Result<()> {
        trace!("reregister {:?} {:?}", token, events);
        unsafe {
            self.binding.reregister_socket(socket, token, poll)?;
        }

        registration
            .lock()
            .unwrap()
            .as_mut()
            .unwrap()
            .reregister(poll, token, events, opts)
    }

    /// Implementation of the `Evented::deregister` function.
    ///
    /// Doesn't allow registration with another event loop, just shuts down
    /// readiness notifications and such.
    pub fn deregister(
        &mut self,
        socket: &dyn AsRawSocket,
        poll: &Poll,
        registration: &Mutex<Option<Registration>>,
    ) -> io::Result<()> {
        trace!("deregistering");
        unsafe {
            self.binding.deregister_socket(socket, poll)?;
        }

        #[allow(deprecated)]
        registration
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .deregister(poll)
    }
}

macro_rules! overlapped2arc {
    ($e:expr, $t:ty, $($field:ident).+) => ({
        let offset = offset_of!($t, $($field).+);
        debug_assert!(offset < mem::size_of::<$t>());
        FromRawArc::from_raw(($e as usize - offset) as *mut $t)
    })
}

macro_rules! offset_of {
    ($t:ty, $($field:ident).+) => (
        &(*(0 as *const $t)).$($field).+ as *const _ as usize
    )
}
