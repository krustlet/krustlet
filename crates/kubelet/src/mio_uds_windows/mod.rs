//! Mio bindings for Unix domain sockets on Windows

#![deny(missing_docs, missing_debug_implementations)]
#![cfg_attr(test, deny(warnings))]

mod listener;
mod poll;
mod stdnet;
mod stream;
mod sys;

pub mod net {
    //! The Windows equivalent of std::os::unix::net
    pub use super::stdnet::{
        AcceptAddrs, AcceptAddrsBuf, SocketAddr, UnixListener, UnixListenerExt, UnixStream,
        UnixStreamExt,
    };
}

pub use listener::UnixListener;
pub use stream::UnixStream;
