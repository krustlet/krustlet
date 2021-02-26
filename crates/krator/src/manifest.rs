use core::pin::Pin;
use core::task::{Context, Poll};
use tokio::sync::watch::{channel, Receiver, Sender};
use tokio_stream::Stream;

/// Wrapper for `ObjectState::Manifest` type which reflects
/// the latest version of the object's manifest.
#[derive(Clone)]
pub struct Manifest<T: Clone> {
    rx: Receiver<T>,
}

impl<T: Clone> Manifest<T> {
    /// Create a new Manifest wrapper from the initial object manifest.
    pub fn new(inner: T) -> (Sender<T>, Self) {
        let (tx, rx) = channel(inner);
        (tx, Manifest { rx })
    }

    /// Obtain a clone of the latest object manifest.
    pub fn latest(&self) -> T {
        self.rx.borrow().clone()
    }
}

impl<T: Clone> Stream for Manifest<T> {
    type Item = T;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        use futures::Future;
        let pending = {
            let mut fut = Box::pin(self.rx.changed());
            Pin::new(&mut fut).poll(cx)
        };
        match pending {
            Poll::Pending => Poll::Pending,
            Poll::Ready(result) => match result {
                Ok(()) => Poll::Ready(Some(self.rx.borrow().clone())),
                Err(_) => Poll::Ready(None),
            },
        }
    }
}
