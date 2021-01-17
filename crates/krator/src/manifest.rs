use tokio::sync::watch::{channel, Receiver, Sender};

#[derive(Clone)]
/// Wrapper for `ObjectState::Manifest` type which reflects
/// the latest version of the object's manifest.
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

impl<T: Clone> tokio::stream::Stream for Manifest<T> {
    type Item = T;

    fn poll_next(
        mut self: core::pin::Pin<&mut Self>,
        cx: &mut core::task::Context,
    ) -> core::task::Poll<Option<Self::Item>> {
        core::pin::Pin::new(&mut self.rx).poll_next(cx)
    }
}
