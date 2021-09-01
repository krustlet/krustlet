use std::{
    path::{Path, PathBuf},
    pin::Pin,
    task::{Context, Poll},
};

use futures::Stream;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tonic::transport::server::Connected;

#[derive(Debug)]
pub struct UnixStream(tokio::net::UnixStream);

/// A `PathBuf` that will get deleted on drop
struct OwnedPathBuf {
    inner: PathBuf,
}

impl Drop for OwnedPathBuf {
    fn drop(&mut self) {
        if let Err(e) = std::fs::remove_file(&self.inner) {
            eprintln!(
                "cleanup of file {} failed, manual cleanup needed: {}",
                self.inner.display(),
                e
            );
        }
    }
}

pub struct Socket {
    listener: tokio::net::UnixListener,
    _socket_path: OwnedPathBuf,
}

impl Socket {
    pub fn new<P: AsRef<Path> + ?Sized>(path: &P) -> anyhow::Result<Self> {
        let listener = tokio::net::UnixListener::bind(path)?;
        Ok(Socket {
            listener,
            _socket_path: OwnedPathBuf {
                inner: path.as_ref().to_owned(),
            },
        })
    }
}

impl Stream for Socket {
    type Item = Result<UnixStream, std::io::Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match Pin::new(&mut self.listener).poll_accept(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(res) => Poll::Ready(Some(res.map(|(stream, _)| UnixStream(stream)))),
        }
    }
}

#[derive(Clone, Debug)]
pub struct ConnectionData {}

impl Connected for UnixStream {
    type ConnectInfo = ConnectionData;

    fn connect_info(&self) -> Self::ConnectInfo {
        ConnectionData {}
    }
}

impl AsyncRead for UnixStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.0).poll_read(cx, buf)
    }
}

impl AsyncWrite for UnixStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.0).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.0).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.0).poll_shutdown(cx)
    }
}
