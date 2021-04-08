use crate::mio_uds_windows::{UnixListener, UnixStream};
use mio::*;
use std::io::ErrorKind;
use tempfile::Builder;

#[test]
fn test_uds_register_multiple_event_loops() {
    let dir = Builder::new().prefix("uds").tempdir().unwrap();
    let listener = UnixListener::bind(dir.path().join("foo")).unwrap();
    let addr = listener.local_addr().unwrap();

    let poll1 = Poll::new().unwrap();
    poll1
        .register(
            &listener,
            Token(0),
            Ready::readable() | Ready::writable(),
            PollOpt::edge(),
        )
        .unwrap();

    let poll2 = Poll::new().unwrap();

    // Try registering the same socket with the initial one
    let res = poll2.register(
        &listener,
        Token(0),
        Ready::readable() | Ready::writable(),
        PollOpt::edge(),
    );
    assert!(res.is_err());
    assert_eq!(res.unwrap_err().kind(), ErrorKind::Other);

    // Try cloning the socket and registering it again
    let listener2 = listener.try_clone().unwrap();
    let res = poll2.register(
        &listener2,
        Token(0),
        Ready::readable() | Ready::writable(),
        PollOpt::edge(),
    );
    assert!(res.is_err());
    assert_eq!(res.unwrap_err().kind(), ErrorKind::Other);

    // Try the stream
    let stream = UnixStream::connect(&addr.as_pathname().unwrap()).unwrap();

    poll1
        .register(
            &stream,
            Token(1),
            Ready::readable() | Ready::writable(),
            PollOpt::edge(),
        )
        .unwrap();

    let res = poll2.register(
        &stream,
        Token(1),
        Ready::readable() | Ready::writable(),
        PollOpt::edge(),
    );
    assert!(res.is_err());
    assert_eq!(res.unwrap_err().kind(), ErrorKind::Other);

    // Try cloning the socket and registering it again
    let stream2 = stream.try_clone().unwrap();
    let res = poll2.register(
        &stream2,
        Token(1),
        Ready::readable() | Ready::writable(),
        PollOpt::edge(),
    );
    assert!(res.is_err());
    assert_eq!(res.unwrap_err().kind(), ErrorKind::Other);
}
