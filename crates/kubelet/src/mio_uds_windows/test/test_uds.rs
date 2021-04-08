use std::cmp;
use std::io;
use std::io::prelude::*;
use std::sync::mpsc::channel;
use std::thread;
use std::time::Duration;
use tempfile::Builder;

use crate::mio_uds_windows::{net, UnixListener, UnixStream};
use iovec::IoVec;
use mio::{Events, Poll, PollOpt, Ready, Token};
use {TryRead, TryWrite};

#[test]
fn accept() {
    struct H {
        hit: bool,
        listener: UnixListener,
        shutdown: bool,
    }
    let dir = Builder::new().prefix("uds").tempdir().unwrap();

    let l = UnixListener::bind(dir.path().join("foo")).unwrap();
    let addr = l.local_addr().unwrap();

    let t = thread::spawn(move || {
        net::UnixStream::connect(&addr.as_pathname().unwrap()).unwrap();
    });

    let poll = Poll::new().unwrap();

    poll.register(&l, Token(1), Ready::readable(), PollOpt::edge())
        .unwrap();

    let mut events = Events::with_capacity(128);

    let mut h = H {
        hit: false,
        listener: l,
        shutdown: false,
    };
    while !h.shutdown {
        poll.poll(&mut events, None).unwrap();

        for event in &events {
            h.hit = true;
            assert_eq!(event.token(), Token(1));
            assert!(event.readiness().is_readable());
            assert!(h.listener.accept().is_ok());
            h.shutdown = true;
        }
    }
    assert!(h.hit);
    assert!(h.listener.accept().unwrap().is_none());
    t.join().unwrap();
}

#[test]
fn connect() {
    struct H {
        hit: u32,
        shutdown: bool,
    }
    let dir = Builder::new().prefix("uds").tempdir().unwrap();

    let l = net::UnixListener::bind(dir.path().join("foo")).unwrap();
    let addr = l.local_addr().unwrap();

    let (tx, rx) = channel();
    let (tx2, rx2) = channel();
    let t = thread::spawn(move || {
        let s = l.accept().unwrap();
        rx.recv().unwrap();
        drop(s);
        tx2.send(()).unwrap();
    });

    let poll = Poll::new().unwrap();
    let s = UnixStream::connect(&addr.as_pathname().unwrap()).unwrap();

    poll.register(
        &s,
        Token(1),
        Ready::readable() | Ready::writable(),
        PollOpt::edge(),
    )
    .unwrap();

    let mut events = Events::with_capacity(128);

    let mut h = H {
        hit: 0,
        shutdown: false,
    };
    while !h.shutdown {
        poll.poll(&mut events, None).unwrap();

        for event in &events {
            assert_eq!(event.token(), Token(1));
            match h.hit {
                0 => assert!(event.readiness().is_writable()),
                1 => assert!(event.readiness().is_readable()),
                _ => panic!(),
            }
            h.hit += 1;
            h.shutdown = true;
        }
    }
    assert_eq!(h.hit, 1);
    tx.send(()).unwrap();
    rx2.recv().unwrap();
    h.shutdown = false;
    while !h.shutdown {
        poll.poll(&mut events, None).unwrap();

        for event in &events {
            assert_eq!(event.token(), Token(1));
            match h.hit {
                0 => assert!(event.readiness().is_writable()),
                1 => assert!(event.readiness().is_readable()),
                _ => panic!(),
            }
            h.hit += 1;
            h.shutdown = true;
        }
    }
    assert_eq!(h.hit, 2);
    t.join().unwrap();
}

#[test]
fn read() {
    const N: usize = 16 * 1024 * 1024;
    struct H {
        amt: usize,
        socket: UnixStream,
        shutdown: bool,
    }
    let dir = Builder::new().prefix("uds").tempdir().unwrap();

    let l = net::UnixListener::bind(dir.path().join("foo")).unwrap();
    let addr = l.local_addr().unwrap();

    let t = thread::spawn(move || {
        let mut s = l.accept().unwrap().0;
        let b = [0; 1024];
        let mut amt = 0;
        while amt < N {
            amt += s.write(&b).unwrap();
        }
    });

    let poll = Poll::new().unwrap();
    let s = UnixStream::connect(&addr.as_pathname().unwrap()).unwrap();

    poll.register(&s, Token(1), Ready::readable(), PollOpt::edge())
        .unwrap();

    let mut events = Events::with_capacity(128);

    let mut h = H {
        amt: 0,
        socket: s,
        shutdown: false,
    };
    while !h.shutdown {
        poll.poll(&mut events, None).unwrap();

        for event in &events {
            assert_eq!(event.token(), Token(1));
            let mut b = [0; 1024];
            loop {
                if let Some(amt) = h.socket.try_read(&mut b).unwrap() {
                    h.amt += amt;
                } else {
                    break;
                }
                if h.amt >= N {
                    h.shutdown = true;
                    break;
                }
            }
        }
    }
    t.join().unwrap();
}

#[test]
fn read_bufs() {
    const N: usize = 16 * 1024 * 1024;
    let dir = Builder::new().prefix("uds").tempdir().unwrap();

    let l = net::UnixListener::bind(dir.path().join("foo")).unwrap();
    let addr = l.local_addr().unwrap();

    let t = thread::spawn(move || {
        let mut s = l.accept().unwrap().0;
        let b = [1; 1024];
        let mut amt = 0;
        while amt < N {
            amt += s.write(&b).unwrap();
        }
    });

    let poll = Poll::new().unwrap();
    let mut events = Events::with_capacity(128);

    let s = UnixStream::connect(&addr.as_pathname().unwrap()).unwrap();

    poll.register(&s, Token(1), Ready::readable(), PollOpt::level())
        .unwrap();

    let b1 = &mut [0; 10][..];
    let b2 = &mut [0; 383][..];
    let b3 = &mut [0; 28][..];
    let b4 = &mut [0; 8][..];
    let b5 = &mut [0; 128][..];
    let mut b: [&mut IoVec; 5] = [b1.into(), b2.into(), b3.into(), b4.into(), b5.into()];

    let mut so_far = 0;
    loop {
        for buf in b.iter_mut() {
            for byte in buf.as_mut_bytes() {
                *byte = 0;
            }
        }

        poll.poll(&mut events, None).unwrap();

        match s.read_bufs(&mut b) {
            Ok(0) => {
                assert_eq!(so_far, N);
                break;
            }
            Ok(mut n) => {
                so_far += n;
                for buf in b.iter() {
                    let buf = buf.as_bytes();
                    for byte in buf[..cmp::min(n, buf.len())].iter() {
                        assert_eq!(*byte, 1);
                    }
                    n = n.saturating_sub(buf.len());
                    if n == 0 {
                        break;
                    }
                }
                assert_eq!(n, 0);
            }
            Err(e) => assert_eq!(e.kind(), io::ErrorKind::WouldBlock),
        }
    }

    t.join().unwrap();
}

#[test]
fn write() {
    const N: usize = 16 * 1024 * 1024;
    struct H {
        amt: usize,
        socket: UnixStream,
        shutdown: bool,
    }
    let dir = Builder::new().prefix("uds").tempdir().unwrap();

    let l = net::UnixListener::bind(dir.path().join("foo")).unwrap();
    let addr = l.local_addr().unwrap();

    let t = thread::spawn(move || {
        let mut s = l.accept().unwrap().0;
        let mut b = [0; 1024];
        let mut amt = 0;
        while amt < N {
            amt += s.read(&mut b).unwrap();
        }
    });

    let poll = Poll::new().unwrap();
    let s = UnixStream::connect(&addr.as_pathname().unwrap()).unwrap();

    poll.register(&s, Token(1), Ready::writable(), PollOpt::edge())
        .unwrap();

    let mut events = Events::with_capacity(128);

    let mut h = H {
        amt: 0,
        socket: s,
        shutdown: false,
    };
    while !h.shutdown {
        poll.poll(&mut events, None).unwrap();

        for event in &events {
            assert_eq!(event.token(), Token(1));
            let b = [0; 1024];
            loop {
                if let Some(amt) = h.socket.try_write(&b).unwrap() {
                    h.amt += amt;
                } else {
                    break;
                }
                if h.amt >= N {
                    h.shutdown = true;
                    break;
                }
            }
        }
    }
    t.join().unwrap();
}

#[test]
fn write_bufs() {
    const N: usize = 16 * 1024 * 1024;
    let dir = Builder::new().prefix("uds").tempdir().unwrap();

    let l = net::UnixListener::bind(dir.path().join("foo")).unwrap();
    let addr = l.local_addr().unwrap();

    let t = thread::spawn(move || {
        let mut s = l.accept().unwrap().0;
        let mut b = [0; 1024];
        let mut amt = 0;
        while amt < N {
            for byte in b.iter_mut() {
                *byte = 0;
            }
            let n = s.read(&mut b).unwrap();
            amt += n;
            for byte in b[..n].iter() {
                assert_eq!(*byte, 1);
            }
        }
    });

    let poll = Poll::new().unwrap();
    let mut events = Events::with_capacity(128);
    let s = UnixStream::connect(&addr.as_pathname().unwrap()).unwrap();
    poll.register(&s, Token(1), Ready::writable(), PollOpt::level())
        .unwrap();

    let b1 = &[1; 10][..];
    let b2 = &[1; 383][..];
    let b3 = &[1; 28][..];
    let b4 = &[1; 8][..];
    let b5 = &[1; 128][..];
    let b: [&IoVec; 5] = [b1.into(), b2.into(), b3.into(), b4.into(), b5.into()];

    let mut so_far = 0;
    while so_far < N {
        poll.poll(&mut events, None).unwrap();

        match s.write_bufs(&b) {
            Ok(n) => so_far += n,
            Err(e) => assert_eq!(e.kind(), io::ErrorKind::WouldBlock),
        }
    }

    t.join().unwrap();
}

#[test]
fn connect_then_close() {
    struct H {
        listener: UnixListener,
        shutdown: bool,
    }
    let dir = Builder::new().prefix("uds").tempdir().unwrap();

    let poll = Poll::new().unwrap();
    let l = UnixListener::bind(dir.path().join("foo")).unwrap();
    let addr = l.local_addr().unwrap();
    let s = UnixStream::connect(&addr.as_pathname().unwrap()).unwrap();

    poll.register(&l, Token(1), Ready::readable(), PollOpt::edge())
        .unwrap();
    poll.register(&s, Token(2), Ready::readable(), PollOpt::edge())
        .unwrap();

    let mut events = Events::with_capacity(128);

    let mut h = H {
        listener: l,
        shutdown: false,
    };
    while !h.shutdown {
        poll.poll(&mut events, None).unwrap();

        for event in &events {
            if event.token() == Token(1) {
                let s = h.listener.accept().unwrap().unwrap().0;
                poll.register(
                    &s,
                    Token(3),
                    Ready::readable() | Ready::writable(),
                    PollOpt::edge(),
                )
                .unwrap();
                drop(s);
            } else if event.token() == Token(2) {
                h.shutdown = true;
            }
        }
    }
}

#[test]
fn listen_then_close() {
    let poll = Poll::new().unwrap();
    let dir = Builder::new().prefix("uds").tempdir().unwrap();
    let l = UnixListener::bind(dir.path().join("foo")).unwrap();

    poll.register(&l, Token(1), Ready::readable(), PollOpt::edge())
        .unwrap();
    drop(l);

    let mut events = Events::with_capacity(128);

    poll.poll(&mut events, Some(Duration::from_millis(100)))
        .unwrap();

    for event in &events {
        if event.token() == Token(1) {
            panic!("recieved ready() on a closed UnixListener")
        }
    }
}

fn assert_send<T: Send>() {}

fn assert_sync<T: Sync>() {}

#[test]
fn test_uds_sockets_are_send() {
    assert_send::<UnixListener>();
    assert_send::<UnixStream>();
    assert_sync::<UnixListener>();
    assert_sync::<UnixStream>();
}

#[test]
fn bind_twice_bad() {
    let dir = Builder::new().prefix("uds").tempdir().unwrap();
    let l1 = UnixListener::bind(dir.path().join("foo")).unwrap();
    let addr = l1.local_addr().unwrap();
    assert!(UnixListener::bind(&addr.as_pathname().unwrap()).is_err());
}

#[test]
fn multiple_writes_immediate_success() {
    const N: usize = 16;
    let dir = Builder::new().prefix("uds").tempdir().unwrap();
    let l = net::UnixListener::bind(dir.path().join("foo")).unwrap();
    let addr = l.local_addr().unwrap();

    let t = thread::spawn(move || {
        let mut s = l.accept().unwrap().0;
        let mut b = [0; 1024];
        let mut amt = 0;
        while amt < 1024 * N {
            for byte in b.iter_mut() {
                *byte = 0;
            }
            let n = s.read(&mut b).unwrap();
            amt += n;
            for byte in b[..n].iter() {
                assert_eq!(*byte, 1);
            }
        }
    });

    let poll = Poll::new().unwrap();
    let mut s = UnixStream::connect(&addr.as_pathname().unwrap()).unwrap();
    poll.register(&s, Token(1), Ready::writable(), PollOpt::level())
        .unwrap();
    let mut events = Events::with_capacity(16);

    // Wait for our UDS stream to connect
    'outer: loop {
        poll.poll(&mut events, None).unwrap();
        for event in events.iter() {
            if event.token() == Token(1) && event.readiness().is_writable() {
                break 'outer;
            }
        }
    }

    for _ in 0..N {
        s.write(&[1; 1024]).unwrap();
    }

    t.join().unwrap();
}

#[test]
fn connection_reset_by_peer() {
    let poll = Poll::new().unwrap();
    let mut events = Events::with_capacity(16);
    let mut buf = [0u8; 16];
    let dir = Builder::new().prefix("uds").tempdir().unwrap();

    // Create listener
    let l = UnixListener::bind(dir.path().join("foo")).unwrap();
    let addr = l.local_addr().unwrap();

    // Connect client
    let client = net::UnixStream::connect(&addr.as_pathname().unwrap()).unwrap();

    // Convert to Mio stream
    let client = UnixStream::from_stream(client).unwrap();

    // Register server
    poll.register(&l, Token(0), Ready::readable(), PollOpt::edge())
        .unwrap();

    // Register interest in the client
    poll.register(
        &client,
        Token(1),
        Ready::readable() | Ready::writable(),
        PollOpt::edge(),
    )
    .unwrap();

    // Wait for listener to be ready
    let mut server;
    'outer: loop {
        poll.poll(&mut events, None).unwrap();

        for event in &events {
            if event.token() == Token(0) {
                match l.accept() {
                    Ok(Some((sock, _))) => {
                        server = sock;
                        break 'outer;
                    }
                    Ok(None) => {}
                    Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {}
                    Err(e) => panic!("unexpected error {:?}", e),
                }
            }
        }
    }

    // Close the connection
    drop(client);

    // Wait a moment
    thread::sleep(Duration::from_millis(100));

    // Register interest in the server socket
    poll.register(&server, Token(3), Ready::readable(), PollOpt::edge())
        .unwrap();

    loop {
        poll.poll(&mut events, None).unwrap();

        for event in &events {
            if event.token() == Token(3) {
                assert!(event.readiness().is_readable());

                match server.read(&mut buf) {
                    Ok(0) | Err(_) => {}

                    Ok(x) => panic!("expected empty buffer but read {} bytes", x),
                }
                return;
            }
        }
    }
}

#[test]
fn connect_error() {
    let poll = Poll::new().unwrap();
    let dir = Builder::new().prefix("uds").tempdir().unwrap();

    // This test is structured differently from the test
    // 'test_tcp::connect_error' in the mio codebase because
    // UnixStream::connect() seems to behave differently from
    // TcpStream::connect() in this case. Specifically, an error
    // with kind == io::ErrorKind::ConnectionRefused is returned
    // from poll.register() rather than poll.poll(). Is that ok?

    let l = UnixStream::connect(&dir.path().join("foo")).unwrap();
    let e = poll.register(&l, Token(0), Ready::writable(), PollOpt::edge());
    assert!(e.is_err());
    assert_eq!(e.err().unwrap().kind(), io::ErrorKind::ConnectionRefused);
}

#[test]
fn write_error() {
    let poll = Poll::new().unwrap();
    let mut events = Events::with_capacity(16);
    let (tx, rx) = channel();
    let dir = Builder::new().prefix("uds").tempdir().unwrap();

    let listener = net::UnixListener::bind(dir.path().join("foo")).unwrap();
    let addr = listener.local_addr().unwrap();
    let t = thread::spawn(move || {
        let (conn, _addr) = listener.accept().unwrap();
        rx.recv().unwrap();
        drop(conn);
    });

    let mut s = UnixStream::connect(&addr.as_pathname().unwrap()).unwrap();
    poll.register(
        &s,
        Token(0),
        Ready::readable() | Ready::writable(),
        PollOpt::edge(),
    )
    .unwrap();

    let mut wait_writable = || 'outer: loop {
        poll.poll(&mut events, None).unwrap();

        for event in &events {
            if event.token() == Token(0) && event.readiness().is_writable() {
                break 'outer;
            }
        }
    };

    wait_writable();

    tx.send(()).unwrap();
    t.join().unwrap();

    let buf = [0; 1024];
    loop {
        match s.write(&buf) {
            Ok(_) => {}
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => wait_writable(),
            Err(e) => {
                println!("good error: {}", e);
                break;
            }
        }
    }
}
