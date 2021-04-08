extern crate mio;

use crate::mio_uds_windows::UnixListener;
use mio::{Events, Poll, PollOpt, Ready, Token};
use std::time::Duration;
use tempfile::Builder;

#[test]
fn run_once_with_nothing() {
    let mut events = Events::with_capacity(1024);
    let poll = Poll::new().unwrap();
    poll.poll(&mut events, Some(Duration::from_millis(100)))
        .unwrap();
}

#[test]
fn add_then_drop() {
    let mut events = Events::with_capacity(1024);
    let dir = Builder::new().prefix("uds").tempdir().unwrap();
    let l = UnixListener::bind(dir.path().join("foo")).unwrap();
    let poll = Poll::new().unwrap();
    poll.register(
        &l,
        Token(1),
        Ready::readable() | Ready::writable(),
        PollOpt::edge(),
    )
    .unwrap();
    drop(l);
    poll.poll(&mut events, Some(Duration::from_millis(100)))
        .unwrap();
}
