use core::pin::Pin;
use core::task::{Context, Poll};
use tokio::stream::Stream;
use tokio::sync::watch::{channel, Receiver, Sender};

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
        Pin::new(&mut self.rx).poll_next(cx)
    }
}
