use anyhow::bail;
use log::{debug, error};
use tokio::io::{AsyncBufReadExt, AsyncRead};

/// Sender for streaming logs to client.
pub struct LogSender {
    sender: Option<hyper::body::Sender>,
}

/// Possible errors sending log data.
#[derive(Debug)]
pub enum LogSendError {
    /// Client has disconnected.
    ChannelClosed,
    /// An unexpected error occured.
    Abnormal(anyhow::Error),
}

impl std::fmt::Display for LogSendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LogSendError::ChannelClosed => write!(f, "ChannelClosed"),
            LogSendError::Abnormal(e) => write!(f, "{}", e),
        }
    }
}

impl std::error::Error for LogSendError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            LogSendError::ChannelClosed => None,
            LogSendError::Abnormal(e) => Some(e.root_cause()),
        }
    }
}

impl LogSender {
    /// Create new `LogSender` from `hyper::body::Sender`.
    pub fn new(sender: hyper::body::Sender) -> Self {
        LogSender {
            sender: Some(sender),
        }
    }

    /// Async send some data to a client.
    pub async fn send(&mut self, data: String) -> Result<(), LogSendError> {
        let b = hyper::body::Bytes::copy_from_slice(&data.as_bytes());
        match self.sender {
            Some(ref mut sender) => sender.send_data(b).await.map_err(|e| {
                if e.is_closed() {
                    LogSendError::ChannelClosed
                } else {
                    LogSendError::Abnormal(anyhow::Error::new(e))
                }
            }),
            None => Err(LogSendError::ChannelClosed),
        }
    }

    /// Gracefully close the channel.
    pub fn close(&mut self) {
        match self.sender.take() {
            Some(sender) => sender.abort(),
            None => (),
        }
    }
}

/// Future that streams logs from provided `AsyncRead` to provided `hyper::body::Sender`.
pub async fn stream_logs<R: AsyncRead + std::marker::Unpin>(
    output: R,
    mut sender: LogSender,
    tail: Option<usize>,
    follow: bool,
) -> anyhow::Result<()> {
    let buf = tokio::io::BufReader::new(output);
    let mut lines = buf.lines();

    if let Some(n) = tail {
        // Stream last n lines.
        // TODO: this uses a lot of memory for large n and scans the entire file.
        let mut line_buf = std::collections::VecDeque::with_capacity(n);

        while let Some(line) = match lines.next_line().await {
            Ok(line) => line,
            Err(e) => {
                let err = format!("Error reading from log: {:?}", e);
                error!("{}", &err);
                sender.send(err).await?;
                sender.close();
                bail!(e);
            }
        } {
            if line_buf.len() == n {
                line_buf.pop_front();
            }
            line_buf.push_back(line);
        }

        for mut line in line_buf {
            line.push('\n');
            match sender.send(line).await {
                Ok(_) => (),
                Err(LogSendError::ChannelClosed) => {
                    debug!("channel closed.");
                    return Ok(());
                }
                Err(LogSendError::Abnormal(e)) => {
                    error!("channel error: {}", e);
                    bail!(e);
                }
            }
        }
    } else {
        // Stream entire file.
        while let Some(mut line) = match lines.next_line().await {
            Ok(line) => line,
            Err(e) => {
                let err = format!("Error reading from log: {:?}", e);
                error!("{}", &err);
                sender.send(err).await?;
                sender.close();
                bail!(e);
            }
        } {
            line.push('\n');
            match sender.send(line).await {
                Ok(_) => (),
                Err(LogSendError::ChannelClosed) => {
                    debug!("channel closed.");
                    return Ok(());
                }
                Err(LogSendError::Abnormal(e)) => {
                    error!("channel error: {}", e);
                    bail!(e);
                }
            }
        }
    }

    if follow {
        // Optionally watch file for changes.
        loop {
            while let Some(mut line) = match lines.next_line().await {
                Ok(line) => line,
                Err(e) => {
                    let err = format!("Error reading from log: {:?}", e);
                    error!("{}", &err);
                    sender.send(err).await?;
                    sender.close();
                    bail!(e);
                }
            } {
                line.push('\n');
                match sender.send(line).await {
                    Ok(_) => (),
                    Err(LogSendError::ChannelClosed) => {
                        debug!("channel closed.");
                        return Ok(());
                    }
                    Err(LogSendError::Abnormal(e)) => {
                        error!("channel error: {}", e);
                        bail!(e);
                    }
                }
            }
            tokio::time::delay_for(std::time::Duration::from_millis(500)).await;
        }
    }

    sender.close();
    Ok(())
}
