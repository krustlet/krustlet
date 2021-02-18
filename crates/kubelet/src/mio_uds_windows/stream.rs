use std::fmt;
use std::io::{self, Read, Write};
use std::net::Shutdown;
use std::os::windows::io::{AsRawSocket, FromRawSocket, IntoRawSocket, RawSocket};
use std::path::Path;

use iovec::IoVec;
use mio::{Evented, Poll, PollOpt, Ready, Token};

use super::net::{self, SocketAddr};
use super::poll::SelectorId;
use super::stdnet::{from_path, init, Socket};
use super::sys;

/// A Unix stream socket
///
/// This type represents a `SOCK_STREAM` connection of the `AF_UNIX` family,
/// otherwise known as Unix domain sockets or Unix sockets. This stream is
/// readable/writable and acts similarly to a TCP stream where reads/writes are
/// all in order with respect to the other connected end.
///
/// A `UnixStream` implements the `Read`, `Write`, and `Evented` traits for
/// interoperating with other I/O code.
///
/// Note that the `read` and `write` methods may return an error with the kind
/// of `WouldBlock`, indicating that it's not ready to read/write just yet.
pub struct UnixStream {
    sys: sys::UnixStream,
    selector_id: SelectorId,
}

fn set_nonblocking(stream: &net::UnixStream) -> io::Result<()> {
    stream.set_nonblocking(true)
}

impl UnixStream {
    /// Connects to the socket named by `path`.
    ///
    /// The socket returned may not be readable and/or writable yet, as the
    /// connection may be in progress. The socket should be registered with an
    /// event loop to wait on both of these properties being available.
    ///
    /// This convenience method uses the system's default options when creating
    /// a socket. If fine-grained control over socket creation is desired, you
    /// can use the `net::UnixStream` type to create the socket, and then pass
    /// it to `UnixStream::connect_stream` to transfer ownership into mio and
    /// schedule the connect operation.
    pub fn connect<P: AsRef<Path>>(path: P) -> io::Result<UnixStream> {
        init();
        fn inner(path: &Path) -> io::Result<UnixStream> {
            let sock = Socket::new()?;
            let sock = unsafe { net::UnixStream::from_raw_socket(sock.into_raw_socket()) };
            let addr = from_path(path)?;
            UnixStream::connect_stream(sock, &addr)
        }
        inner(path.as_ref())
    }

    /// Transfers ownership of a previously configured and pending socket into
    /// the returned mio-compatible `UnixStream`, and connects it to the socket
    /// named by `path`.
    ///
    /// The platform specific behavior of this function looks like:
    ///
    /// * On Windows, the path is stored internally and the connect operation
    ///   is issued when the returned `UnixStream` is registered with an event
    ///   loop. Note that on Windows you must `bind` a socket before it can be
    ///   connected, so `stream` must be bound before this method is called.
    pub fn connect_stream(stream: net::UnixStream, addr: &SocketAddr) -> io::Result<UnixStream> {
        Ok(UnixStream {
            sys: sys::UnixStream::connect(stream, addr)?,
            selector_id: SelectorId::new(),
        })
    }

    /// Consumes an already-connected `mio_uds_windows::net::UnixStream` and
    /// returns a wrapped `UnixStream` compatible with mio.
    ///
    /// The returned stream should be ready to be associated with an event loop.
    pub fn from_stream(stream: net::UnixStream) -> io::Result<UnixStream> {
        set_nonblocking(&stream)?;

        Ok(UnixStream {
            sys: sys::UnixStream::from_stream(stream),
            selector_id: SelectorId::new(),
        })
    }

    /// Returns the socket address of the remote half of this connection.
    pub fn peer_addr(&self) -> io::Result<SocketAddr> {
        self.sys.peer_addr()
    }

    /// Returns the socket address of the local half of this connection.
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.sys.local_addr()
    }

    /// Creates a new independently owned handle to the underlying socket.
    ///
    /// The returned `UnixStream` is a reference to the same stream that this
    /// object references. Both handles will read and write the same stream of
    /// data, and options set on one stream will be propagated to the other
    /// stream.
    pub fn try_clone(&self) -> io::Result<UnixStream> {
        self.sys.try_clone().map(|s| UnixStream {
            sys: s,
            selector_id: self.selector_id.clone(),
        })
    }

    /// Shuts down the read, write, or both halves of this connection.
    ///
    /// This function will cause all pending and future I/O on the specified
    /// portions to return immediately with an appropriate value (see the
    /// documentation of `Shutdown`).
    pub fn shutdown(&self, how: Shutdown) -> io::Result<()> {
        self.sys.shutdown(how)
    }

    /// Returns the value of the `SO_ERROR` option on this socket.
    ///
    /// This will retrieve the stored error in the underlying socket, clearing
    /// the field in the process. This can be useful for checking errors between
    /// calls.
    pub fn take_error(&self) -> io::Result<Option<io::Error>> {
        self.sys.take_error()
    }

    /// Read in a list of buffers all at once.
    ///
    /// This operation will attempt to read bytes from this socket and place
    /// them into the list of buffers provided. Note that each buffer is an
    /// `IoVec` which can be created from a byte slice.
    ///
    /// The buffers provided will be filled in sequentially. A buffer will be
    /// entirely filled up before the next is written to.
    ///
    /// The number of bytes read is returned, if successful, or an error is
    /// returned otherwise. If no bytes are available to be read yet then
    /// a "would block" error is returned. This operation does not block.
    pub fn read_bufs(&self, bufs: &mut [&mut IoVec]) -> io::Result<usize> {
        self.sys.readv(bufs)
    }

    /// Write a list of buffers all at once.
    ///
    /// This operation will attempt to write a list of byte buffers to this
    /// socket. Note that each buffer is an `IoVec` which can be created from a
    /// byte slice.
    ///
    /// The buffers provided will be written sequentially. A buffer will be
    /// entirely written before the next is written.
    ///
    /// The number of bytes written is returned, if successful, or an error is
    /// returned otherwise. If the socket is not currently writable then a
    /// "would block" error is returned. This operation does not block.
    pub fn write_bufs(&self, bufs: &[&IoVec]) -> io::Result<usize> {
        self.sys.writev(bufs)
    }
}

impl Read for UnixStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        (&self.sys).read(buf)
    }
}

impl<'a> Read for &'a UnixStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        (&self.sys).read(buf)
    }
}

impl Write for UnixStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        (&self.sys).write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        (&self.sys).flush()
    }
}

impl<'a> Write for &'a UnixStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        (&self.sys).write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        (&self.sys).flush()
    }
}

impl Evented for UnixStream {
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

impl fmt::Debug for UnixStream {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Debug::fmt(&self.sys, f)
    }
}

impl AsRawSocket for UnixStream {
    fn as_raw_socket(&self) -> RawSocket {
        self.sys.as_raw_socket()
    }
}
