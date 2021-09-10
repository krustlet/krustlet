use std::{
    convert::TryFrom,
    fmt::Debug,
    io::{self, Read, Seek, Write},
};

use anyhow::Error;
use serde::{Deserialize, Serialize};
use tokio::fs::File;
use wasmer_wasi::{VirtualFile, WasiFsError};

/// For piping stdio. Stores all output / input in a byte-vector.
pub struct FilePipe {
    file: std::fs::File,
}

impl Debug for FilePipe {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FilePipe")
            .field("file", &self.file)
            .finish()
    }
}

impl Serialize for FilePipe {
    fn serialize<S>(&self, _serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        unimplemented!()
    }
}

impl<'de> Deserialize<'de> for FilePipe {
    fn deserialize<D>(_deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        unimplemented!()
    }
}

impl TryFrom<File> for FilePipe {
    type Error = Error;

    fn try_from(value: File) -> Result<Self, Self::Error> {
        Ok(Self {
            file: value
                .try_into_std()
                .map_err(|_| anyhow::anyhow!("cannot convert tokio file into std file"))?,
        })
    }
}

impl Read for FilePipe {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.file.read(&mut buf.to_vec())
    }
}

impl Write for FilePipe {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.file.write(buf)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

impl Seek for FilePipe {
    fn seek(&mut self, _pos: io::SeekFrom) -> io::Result<u64> {
        Err(io::Error::new(
            io::ErrorKind::Other,
            "can not seek in a FilePipe",
        ))
    }
}

impl VirtualFile for FilePipe {
    fn last_accessed(&self) -> u64 {
        0
    }
    fn last_modified(&self) -> u64 {
        0
    }
    fn created_time(&self) -> u64 {
        0
    }
    fn size(&self) -> u64 {
        0
    }
    fn set_len(&mut self, _len: u64) -> Result<(), WasiFsError> {
        Ok(())
    }
    fn unlink(&mut self) -> Result<(), WasiFsError> {
        Ok(())
    }
    fn bytes_available(&self) -> Result<usize, WasiFsError> {
        let bytes = self.file.metadata()?.len();
        Ok(bytes as usize)
    }
}
