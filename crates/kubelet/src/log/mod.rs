//! `log` contains convenient wrappers around fetching logs from the Kubernetes API.
use anyhow::bail;
use log::{debug, error};
use serde::Deserialize;
use tokio::io::{AsyncBufReadExt, AsyncRead};

/// Possible errors sending log data.
#[derive(Debug)]
pub enum SendError {
    /// Client has disconnected.
    ChannelClosed,
    /// An unexpected error occured.
    Abnormal(anyhow::Error),
}

impl From<std::io::Error> for SendError {
    fn from(error: std::io::Error) -> Self {
        SendError::Abnormal(anyhow::Error::new(error))
    }
}

impl std::fmt::Display for SendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SendError::ChannelClosed => write!(f, "ChannelClosed"),
            SendError::Abnormal(e) => write!(f, "{}", e),
        }
    }
}

impl std::error::Error for SendError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            SendError::ChannelClosed => None,
            SendError::Abnormal(e) => Some(e.root_cause()),
        }
    }
}

#[derive(Debug, Deserialize)]
/// Client options for fetching logs.
pub struct Options {
    /// the number of lines to stream back to the client.
    #[serde(rename = "tailLines")]
    pub tail: Option<usize>,
    /// determines whether the stream should stay open after tailing until the channel has closed.
    #[serde(default)]
    pub follow: bool,
}

/// Sender for streaming logs to client.
pub struct Sender {
    sender: hyper::body::Sender,
    opts: Options,
}

impl Sender {
    /// Create new `Sender` from `hyper::body::Sender`.
    pub fn new(sender: hyper::body::Sender, opts: Options) -> Self {
        Sender { sender, opts }
    }

    /// The tail flag indicated by the request if present.
    pub fn tail(&self) -> Option<usize> {
        self.opts.tail
    }

    /// The follow flag indicated by the request, or `false` if absent.
    pub fn follow(&self) -> bool {
        self.opts.follow
    }

    /// Async send some data to a client.
    pub async fn send(&mut self, data: String) -> Result<(), SendError> {
        let b: hyper::body::Bytes = data.into();
        self.sender.send_data(b).await.map_err(|e| {
            if e.is_closed() {
                debug!("channel closed.");
                SendError::ChannelClosed
            } else {
                error!("channel error: {}", e);
                SendError::Abnormal(anyhow::Error::new(e))
            }
        })
    }
}

/// Stream last `n` lines.
async fn tail<R: AsyncRead + std::marker::Unpin>(
    lines: &mut tokio::io::Lines<tokio::io::BufReader<R>>,
    sender: &mut Sender,
    n: usize,
) -> Result<(), SendError> {
    let mut line_buf = std::collections::VecDeque::with_capacity(n);

    while let Some(line) = match lines.next_line().await {
        Ok(line) => line,
        Err(e) => {
            let err = format!("Error reading from log: {:?}", e);
            error!("{}", &err);
            sender.send(err).await?;
            return Err(e.into());
        }
    } {
        if line_buf.len() == n {
            line_buf.pop_front();
        }
        line_buf.push_back(line);
    }

    for mut line in line_buf {
        line.push('\n');
        sender.send(line).await?;
    }
    Ok(())
}

/// Stream log to end.
async fn stream_to_end<R: AsyncRead + std::marker::Unpin>(
    lines: &mut tokio::io::Lines<tokio::io::BufReader<R>>,
    sender: &mut Sender,
) -> Result<(), SendError> {
    while let Some(mut line) = match lines.next_line().await {
        Ok(line) => line,
        Err(e) => {
            let err = format!("Error reading from log: {:?}", e);
            error!("{}", &err);
            sender.send(err).await?;
            return Err(e.into());
        }
    } {
        line.push('\n');
        sender.send(line).await?;
    }
    Ok(())
}

/// Future that streams logs from provided `AsyncRead` to provided `Sender`.
pub async fn stream<R: AsyncRead + std::marker::Unpin>(
    handle: R,
    mut sender: Sender,
) -> anyhow::Result<()> {
    let buf = tokio::io::BufReader::new(handle);
    let mut lines = buf.lines();

    if let Some(n) = sender.tail() {
        match tail(&mut lines, &mut sender, n).await {
            Ok(_) => (),
            Err(SendError::ChannelClosed) => return Ok(()),
            Err(SendError::Abnormal(e)) => bail!(e),
        }
    } else {
        match stream_to_end(&mut lines, &mut sender).await {
            Ok(_) => (),
            Err(SendError::ChannelClosed) => return Ok(()),
            Err(SendError::Abnormal(e)) => bail!(e),
        }
    }

    if sender.follow() {
        loop {
            match stream_to_end(&mut lines, &mut sender).await {
                Ok(_) => (),
                Err(SendError::ChannelClosed) => return Ok(()),
                Err(SendError::Abnormal(e)) => bail!(e),
            }

            tokio::time::delay_for(std::time::Duration::from_millis(500)).await;
        }
    }

    Ok(())
}

// TODO: Both providers make a handle containing a tempfile. If this is a common pattern,
// it might make sense to provide that implementation here. This would add `tempfile` as a
// dependency of `kubelet`.
/// Trait to describe necessary behavior for creating multiple log readers.
pub trait HandleFactory<R>: Sync + Send {
    /// Create new log reader.
    fn new_handle(&self) -> R;
}
