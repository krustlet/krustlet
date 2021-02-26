use std::fmt;
use std::io;
use std::path::Path;

use mio::{Evented, Poll, PollOpt, Ready, Token};

use super::net::{self, SocketAddr};
use super::poll::SelectorId;
use super::stream::UnixStream;
use super::sys;

/// A Unix domain socket server
///
/// This listener can be used to accept new streams connected to a remote
/// endpoint, through which the `read` and `write` methods can be used to
/// communicate.
pub struct UnixListener {
    sys: sys::UnixListener,
    selector_id: SelectorId,
}

impl UnixListener {
    /// Creates a new `UnixListener` bound to the specified socket.
    pub fn bind<P: AsRef<Path>>(path: P) -> io::Result<UnixListener> {
        let sock = net::UnixListener::bind(path)?;
        UnixListener::from_listener(sock)
    }

    /// Consumes a `mio_uds_windows::net::UnixListener` and returns a wrapped
    /// `UnixListener` compatible with mio.
    ///
    /// The returned listener ready to get associated with an event loop.
    pub fn from_listener(listener: net::UnixListener) -> io::Result<UnixListener> {
        sys::UnixListener::new(listener).map(|s| UnixListener {
            sys: s,
            selector_id: SelectorId::new(),
        })
    }

    /// Accepts a new incoming connection to this listener.
    ///
    /// When established, the corresponding `UnixStream` and the remote peer's
    /// address will be returned as `Ok(Some(...))`. If there is no connection
    /// waiting to be accepted, then `Ok(None)` is returned.
    ///
    /// If an error happens while accepting, `Err` is returned.
    pub fn accept(&self) -> io::Result<Option<(UnixStream, SocketAddr)>> {
        match self.accept_std()? {
            Some((stream, addr)) => Ok(Some((UnixStream::from_stream(stream)?, addr))),
            None => Ok(None),
        }
    }

    /// Accepts a new incoming connection to this listener.
    ///
    /// This method is the same as `accept`, except that it returns a socket *in
    /// blocking mode* which isn't bound to a `mio` type. This can later be
    /// converted to a `mio` type, if necessary.
    ///
    /// If an error happens while accepting, `Err` is returned.
    pub fn accept_std(&self) -> io::Result<Option<(net::UnixStream, SocketAddr)>> {
        match self.sys.accept() {
            Ok((socket, addr)) => Ok(Some((socket, addr))),
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Returns the local socket address of this listener.
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.sys.local_addr()
    }

    /// Creates a new independently owned handle to the underlying socket.
    ///
    /// The returned `UnixListener` is a reference to the same socket that this
    /// object references. Both handles can be used to accept incoming
    /// connections and options set on one listener will affect the other.
    pub fn try_clone(&self) -> io::Result<UnixListener> {
        self.sys.try_clone().map(|s| UnixListener {
            sys: s,
            selector_id: self.selector_id.clone(),
        })
    }

    /// Returns the value of the `SO_ERROR` option on this socket.
    ///
    /// This will retrieve the stored error in the underlying socket, clearing
    /// the field in the process. This can be useful for checking errors between
    /// calls.
    pub fn take_error(&self) -> io::Result<Option<io::Error>> {
        self.sys.take_error()
    }
}

impl Evented for UnixListener {
    fn register(
        &self,
        poll: &Poll,
        token: Token,
        interest: Ready,
        opts: PollOpt,
    ) -> io::Result<()> {
        self.selector_id.associate_selector(poll)?;
        self.sys.register(poll, token, interest, opts)
    }

    fn reregister(
        &self,
        poll: &Poll,
        token: Token,
        interest: Ready,
        opts: PollOpt,
    ) -> io::Result<()> {
        self.sys.reregister(poll, token, interest, opts)
    }

    fn deregister(&self, poll: &Poll) -> io::Result<()> {
        self.sys.deregister(poll)
    }
}

impl fmt::Debug for UnixListener {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Debug::fmt(&self.sys, f)
    }
}
