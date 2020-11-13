mod test_close_on_drop;
mod test_custom_evented;
mod test_double_register;
mod test_echo_server;
mod test_local_addr_ready;
mod test_oneshot;
mod test_poll;
mod test_register_deregister;
mod test_register_multiple_event_loops;
mod test_reregister_without_poll;
mod test_smoke;
mod test_uds;
mod test_uds_level;
mod test_write_then_drop;

use bytes::{Buf, MutBuf};
use mio::event::Event;
use mio::{Events, Poll};
use std::io::{self, Read, Write};
use std::time::Duration;

pub trait TryRead {
    fn try_read_buf<B: MutBuf>(&mut self, buf: &mut B) -> io::Result<Option<usize>>
    where
        Self: Sized,
    {
        // Reads the length of the slice supplied by buf.mut_bytes into the buffer
        // This is not guaranteed to consume an entire datagram or segment.
        // If your protocol is msg based (instead of continuous stream) you should
        // ensure that your buffer is large enough to hold an entire segment (1532 bytes if not jumbo
        // frames)
        let res = self.try_read(unsafe { buf.mut_bytes() });

        if let Ok(Some(cnt)) = res {
            unsafe {
                buf.advance(cnt);
            }
        }

        res
    }

    fn try_read(&mut self, buf: &mut [u8]) -> io::Result<Option<usize>>;
}

pub trait TryWrite {
    fn try_write_buf<B: Buf>(&mut self, buf: &mut B) -> io::Result<Option<usize>>
    where
        Self: Sized,
    {
        let res = self.try_write(buf.bytes());

        if let Ok(Some(cnt)) = res {
            buf.advance(cnt);
        }

        res
    }

    fn try_write(&mut self, buf: &[u8]) -> io::Result<Option<usize>>;
}

impl<T: Read> TryRead for T {
    fn try_read(&mut self, dst: &mut [u8]) -> io::Result<Option<usize>> {
        self.read(dst).map_non_block()
    }
}

impl<T: Write> TryWrite for T {
    fn try_write(&mut self, src: &[u8]) -> io::Result<Option<usize>> {
        self.write(src).map_non_block()
    }
}

/*
 *
 * ===== Helpers =====
 *
 */

/// A helper trait to provide the map_non_block function on Results.
trait MapNonBlock<T> {
    /// Maps a `Result<T>` to a `Result<Option<T>>` by converting
    /// operation-would-block errors into `Ok(None)`.
    fn map_non_block(self) -> io::Result<Option<T>>;
}

impl<T> MapNonBlock<T> for io::Result<T> {
    fn map_non_block(self) -> io::Result<Option<T>> {
        use std::io::ErrorKind::WouldBlock;

        match self {
            Ok(value) => Ok(Some(value)),
            Err(err) => {
                if let WouldBlock = err.kind() {
                    Ok(None)
                } else {
                    Err(err)
                }
            }
        }
    }
}

pub fn sleep_ms(ms: u64) {
    use std::thread;
    use std::time::Duration;
    thread::sleep(Duration::from_millis(ms));
}

pub fn expect_events(
    poll: &Poll,
    event_buffer: &mut Events,
    poll_try_count: usize,
    mut expected: Vec<Event>,
) {
    const MS: u64 = 1_000;

    for _ in 0..poll_try_count {
        poll.poll(event_buffer, Some(Duration::from_millis(MS)))
            .unwrap();
        for event in event_buffer.iter() {
            let pos_opt = match expected.iter().position(|exp_event| {
                (event.token() == exp_event.token())
                    && event.readiness().contains(exp_event.readiness())
            }) {
                Some(x) => Some(x),
                None => None,
            };
            if let Some(pos) = pos_opt {
                expected.remove(pos);
            }
        }

        if expected.len() == 0 {
            break;
        }
    }

    assert!(
        expected.len() == 0,
        "The following expected events were not found: {:?}",
        expected
    );
}
