use crate::mio_uds_windows::{UnixListener, UnixStream};
use mio::*;
use sleep_ms;
use std::time::Duration;
use tempfile::Builder;

const MS: u64 = 1_000;

#[test]
pub fn test_reregister_different_without_poll() {
    let mut events = Events::with_capacity(1024);
    let poll = Poll::new().unwrap();
    let dir = Builder::new().prefix("uds").tempdir().unwrap();

    // Create the listener
    let l = UnixListener::bind(dir.path().join("foo")).unwrap();
    let addr = l.local_addr().unwrap();

    // Register the listener with `Poll`
    poll.register(
        &l,
        Token(0),
        Ready::readable(),
        PollOpt::edge() | PollOpt::oneshot(),
    )
    .unwrap();

    let s1 = UnixStream::connect(&addr.as_pathname().unwrap()).unwrap();
    poll.register(&s1, Token(2), Ready::readable(), PollOpt::edge())
        .unwrap();

    sleep_ms(MS);

    poll.reregister(
        &l,
        Token(0),
        Ready::writable(),
        PollOpt::edge() | PollOpt::oneshot(),
    )
    .unwrap();

    poll.poll(&mut events, Some(Duration::from_millis(MS)))
        .unwrap();
    assert_eq!(events.len(), 0);
}
