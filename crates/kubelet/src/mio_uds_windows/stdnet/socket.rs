#![allow(non_camel_case_types)]

use std::io;
use std::mem;
use std::net::Shutdown;
use std::os::raw::{c_int, c_ulong};
use std::os::windows::io::{AsRawSocket, FromRawSocket, IntoRawSocket, RawSocket};
use std::ptr;
use std::sync::Once;

use super::{cvt, last_error};

use kernel32::{GetCurrentProcessId, SetHandleInformation};
use winapi::{
    socklen_t, AF_UNIX, DWORD, FIONBIO, HANDLE, INVALID_SOCKET, SOCKADDR, SOCKET, SOCK_STREAM,
    SOL_SOCKET, SO_ERROR, WSADATA, WSAPROTOCOL_INFOW,
};
// use winapi::WSACleanup;
use ws2_32::getsockopt as c_getsockopt;
use ws2_32::{
    accept, closesocket, ioctlsocket, recv, send, shutdown, WSADuplicateSocketW, WSASocketW,
    WSAStartup,
};

pub const WSA_FLAG_OVERLAPPED: DWORD = 0x01;
pub const HANDLE_FLAG_INHERIT: DWORD = 0x01;
pub const SD_RECEIVE: c_int = 0x00;
pub const SD_SEND: c_int = 0x01;
pub const SD_BOTH: c_int = 0x02;

#[derive(Debug)]
pub struct Socket(SOCKET);

/// Checks whether the Windows socket interface has been started already, and
/// if not, starts it.
pub fn init() {
    static START: Once = Once::new();

    START.call_once(|| unsafe {
        let mut data: WSADATA = mem::zeroed();
        let ret = WSAStartup(
            0x202, // version 2.2
            &mut data,
        );
        assert_eq!(ret, 0);

        // let _ = std::rt::at_exit(|| { WSACleanup(); });
    });
}

#[doc(hidden)]
pub trait IsZero {
    fn is_zero(&self) -> bool;
}

macro_rules! impl_is_zero {
    ($($t:ident)*) => ($(impl IsZero for $t {
        fn is_zero(&self) -> bool {
            *self == 0
        }
    })*)
}

impl_is_zero! { i8 i16 i32 i64 isize u8 u16 u32 u64 usize }

fn cvt_z<I: IsZero>(i: I) -> io::Result<I> {
    if i.is_zero() {
        Err(io::Error::last_os_error())
    } else {
        Ok(i)
    }
}

impl Socket {
    pub fn new() -> io::Result<Socket> {
        let socket = unsafe {
            match WSASocketW(
                AF_UNIX,
                SOCK_STREAM,
                0,
                ptr::null_mut(),
                0,
                WSA_FLAG_OVERLAPPED,
            ) {
                INVALID_SOCKET => Err(last_error()),
                n => Ok(Socket(n)),
            }
        }?;
        socket.set_no_inherit()?;
        Ok(socket)
    }

    // socketpair() not supported on Windows
    // pub fn new_pair(fam: c_int, ty: c_int) -> io::Result<(Socket, Socket)> { ... }

    pub fn accept(&self, storage: *mut SOCKADDR, len: *mut c_int) -> io::Result<Socket> {
        let socket = unsafe {
            match accept(self.0, storage, len) {
                INVALID_SOCKET => Err(last_error()),
                n => Ok(Socket(n)),
            }
        }?;
        socket.set_no_inherit()?;
        Ok(socket)
    }

    pub fn duplicate(&self) -> io::Result<Socket> {
        let socket = unsafe {
            let mut info: WSAPROTOCOL_INFOW = mem::zeroed();
            cvt(WSADuplicateSocketW(
                self.0,
                GetCurrentProcessId(),
                &mut info,
            ))?;
            match WSASocketW(
                info.iAddressFamily,
                info.iSocketType,
                info.iProtocol,
                &mut info,
                0,
                WSA_FLAG_OVERLAPPED,
            ) {
                INVALID_SOCKET => Err(last_error()),
                n => Ok(Socket(n)),
            }
        }?;
        socket.set_no_inherit()?;
        Ok(socket)
    }

    fn recv_with_flags(&self, buf: &mut [u8], flags: c_int) -> io::Result<usize> {
        let ret = cvt(unsafe {
            recv(
                self.0,
                buf.as_mut_ptr() as *mut _,
                buf.len() as c_int,
                flags,
            )
        })?;
        Ok(ret as usize)
    }

    pub fn read(&self, buf: &mut [u8]) -> io::Result<usize> {
        self.recv_with_flags(buf, 0)
    }

    pub fn write(&self, buf: &[u8]) -> io::Result<usize> {
        let ret = cvt(unsafe { send(self.0, buf as *const _ as *const _, buf.len() as c_int, 0) })?;
        Ok(ret as usize)
    }

    fn set_no_inherit(&self) -> io::Result<()> {
        cvt_z(unsafe { SetHandleInformation(self.0 as HANDLE, HANDLE_FLAG_INHERIT, 0) }).map(|_| ())
    }

    pub fn set_nonblocking(&self, nonblocking: bool) -> io::Result<()> {
        let mut nonblocking = nonblocking as c_ulong;
        let r = unsafe { ioctlsocket(self.0, FIONBIO as c_int, &mut nonblocking) };
        if r == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }

    pub fn shutdown(&self, how: Shutdown) -> io::Result<()> {
        let how = match how {
            Shutdown::Write => SD_SEND,
            Shutdown::Read => SD_RECEIVE,
            Shutdown::Both => SD_BOTH,
        };
        cvt(unsafe { shutdown(self.0, how) })?;
        Ok(())
    }

    pub fn take_error(&self) -> io::Result<Option<io::Error>> {
        let raw: c_int = getsockopt(self, SOL_SOCKET, SO_ERROR)?;
        if raw == 0 {
            Ok(None)
        } else {
            Ok(Some(io::Error::from_raw_os_error(raw as i32)))
        }
    }
}

pub fn getsockopt<T: Copy>(sock: &Socket, opt: c_int, val: c_int) -> io::Result<T> {
    unsafe {
        let mut slot: T = mem::zeroed();
        let mut len = mem::size_of::<T>() as socklen_t;
        cvt(c_getsockopt(
            sock.as_raw_socket() as _,
            opt,
            val,
            &mut slot as *mut _ as *mut _,
            &mut len,
        ))?;
        assert_eq!(len as usize, mem::size_of::<T>());
        Ok(slot)
    }
}

impl Drop for Socket {
    fn drop(&mut self) {
        let _ = unsafe { closesocket(self.0) };
    }
}

impl AsRawSocket for Socket {
    fn as_raw_socket(&self) -> RawSocket {
        self.0 as RawSocket
    }
}

impl FromRawSocket for Socket {
    unsafe fn from_raw_socket(sock: RawSocket) -> Self {
        Socket(sock as SOCKET)
    }
}

impl IntoRawSocket for Socket {
    fn into_raw_socket(self) -> RawSocket {
        let ret = self.0 as RawSocket;
        mem::forget(self);
        ret
    }
}
