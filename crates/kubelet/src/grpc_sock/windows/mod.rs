// This is a modified version of: https://github.com/hyperium/tonic/blob/f1275b611e38ec5fe992b2f10552bf95e8448b17/examples/src/uds/server.rs

use std::{
    path::Path,
    pin::Pin,
    task::{Context, Poll},
};

use futures::{FutureExt, Stream};
use mio::Ready;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio_compat_02::FutureExt as CompatFutureExt;
use tonic::transport::server::Connected;

pub struct UnixStream {
    inner: tokio_compat_02::IoCompat<tokio_02::io::PollEvented<crate::mio_uds_windows::UnixStream>>,
}

impl UnixStream {
    pub fn new(stream: crate::mio_uds_windows::UnixStream) -> Result<UnixStream, std::io::Error> {
        let inner = match async {
            Ok::<_, std::io::Error>(tokio_compat_02::IoCompat::new(
                tokio_02::io::PollEvented::new(stream)?,
            ))
        }
        .compat()
        .now_or_never()
        {
            Some(res) => res?,
            None => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "Unable to start IO poll",
                ))
            }
        };
        Ok(UnixStream { inner })
    }
}

pub struct Socket {
    listener: tokio_02::io::PollEvented<crate::mio_uds_windows::UnixListener>,
}

impl Socket {
    #[allow(dead_code)]
    pub fn new<P: AsRef<Path>>(path: &P) -> anyhow::Result<Self> {
        let p = path.as_ref().to_owned();
        let listener = crate::mio_uds_windows::UnixListener::bind(p)?;
        let listener = match async { tokio_02::io::PollEvented::new(listener) }
            .compat()
            .now_or_never()
        {
            Some(res) => res?,
            None => return Err(anyhow::anyhow!("Unable to poll IO")),
        };
        Ok(Socket { listener })
    }
}

impl Stream for Socket {
    type Item = Result<UnixStream, std::io::Error>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        futures::ready!(self.listener.poll_read_ready(cx, Ready::readable()))?;

        let stream = match self.listener.get_ref().accept() {
            Ok(None) => {
                self.listener.clear_read_ready(cx, Ready::readable())?;
                return Poll::Pending;
            }
            Ok(Some((stream, _))) => stream,
            // Not much error handling we can do here, so just return Pending so
            // it'll try again
            Err(_) => {
                self.listener.clear_read_ready(cx, Ready::readable())?;
                return Poll::Pending;
            }
        };
        Poll::Ready(Some(UnixStream::new(stream)))
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
