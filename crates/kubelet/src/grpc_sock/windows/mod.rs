// This is a modified version of: https://github.com/hyperium/tonic/blob/f1275b611e38ec5fe992b2f10552bf95e8448b17/examples/src/uds/server.rs

use std::{
    path::Path,
    pin::Pin,
    task::{Context, Poll},
};

use futures::stream::TryStreamExt;
use futures::Stream;
use tokio::io::{AsyncRead, AsyncWrite};
use tonic::transport::server::Connected;

#[derive(Debug)]
pub struct UnixStream {
    inner: tokio::io::PollEvented<mio_uds_windows::UnixStream>,
}

impl UnixStream {
    pub fn new(stream: mio_uds_windows::UnixStream) -> Self {
        return UnixStream { inner: stream };
    }
}

pub struct Socket {
    listener: mio_uds_windows::UnixListener,
}

impl Socket {
    pub fn new<P: AsRef<Path>>(path: &P) -> anyhow::Result<Self> {
        let listener = mio_uds_windows::UnixListener::bind(path)?;
        Ok(Socket { listener })
    }
}

impl Stream for Socket {
    type Item = Result<UnixStream, std::io::Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut stream = self.listener.incoming().map_ok(UnixStream::new);
        Pin::new(&mut stream).poll_next(cx)
    }
}

impl Connected for UnixStream {}

impl AsyncRead for UnixStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.inner).poll_read(cx, buf)
    }
}

impl AsyncWrite for UnixStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}
