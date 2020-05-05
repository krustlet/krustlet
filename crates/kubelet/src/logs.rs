use anyhow::bail;
use log::{debug, error};
use tokio::io::{AsyncBufReadExt, AsyncRead};

/// Sender for streaming logs to client.
pub struct LogSender {
    sender: Option<hyper::body::Sender>,
    tail: Option<usize>,
    follow: bool,
}

/// Possible errors sending log data.
#[derive(Debug)]
pub enum LogSendError {
    /// Client has disconnected.
    ChannelClosed,
    /// An unexpected error occured.
    Abnormal(anyhow::Error),
}

impl From<std::io::Error> for LogSendError {
    fn from(error: std::io::Error) -> Self {
        LogSendError::Abnormal(anyhow::Error::new(error))
    }
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
    pub fn new(sender: hyper::body::Sender, tail: Option<usize>, follow: bool) -> Self {
        LogSender {
            sender: Some(sender),
            tail,
            follow,
        }
    }

    /// The tail flag indicated by the request if present.
    pub fn tail(&self) -> Option<usize> {
        self.tail.clone()
    }

    /// The follow flag indicated by the request, or `false` if absent.
    pub fn follow(&self) -> bool {
        self.follow
    }

    /// Async send some data to a client.
    pub async fn send(&mut self, data: String) -> Result<(), LogSendError> {
        let b: hyper::body::Bytes = data.into();
        match self.sender {
            Some(ref mut sender) => sender.send_data(b).await.map_err(|e| {
                if e.is_closed() {
                    debug!("channel closed.");
                    LogSendError::ChannelClosed
                } else {
                    error!("channel error: {}", e);
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

/// Stream last `n` lines.
async fn tail_logs<R: AsyncRead + std::marker::Unpin>(
    lines: &mut tokio::io::Lines<tokio::io::BufReader<R>>,
    sender: &mut LogSender,
    n: usize,
) -> Result<(), LogSendError> {
    let mut line_buf = std::collections::VecDeque::with_capacity(n);

    while let Some(line) = match lines.next_line().await {
        Ok(line) => line,
        Err(e) => {
            let err = format!("Error reading from log: {:?}", e);
            error!("{}", &err);
            sender.send(err).await?;
            sender.close();
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
    sender: &mut LogSender,
) -> Result<(), LogSendError> {
    while let Some(mut line) = match lines.next_line().await {
        Ok(line) => line,
        Err(e) => {
            let err = format!("Error reading from log: {:?}", e);
            error!("{}", &err);
            sender.send(err).await?;
            sender.close();
            return Err(e.into());
        }
    } {
        line.push('\n');
        sender.send(line).await?;
    }
    Ok(())
}

/// Future that streams logs from provided `AsyncRead` to provided `LogSender`.
pub async fn stream_logs<R: AsyncRead + std::marker::Unpin>(
    handle: R,
    mut sender: LogSender,
) -> anyhow::Result<()> {
    let buf = tokio::io::BufReader::new(handle);
    let mut lines = buf.lines();

    if let Some(n) = sender.tail() {
        match tail_logs(&mut lines, &mut sender, n).await {
            Ok(_) => (),
            Err(LogSendError::ChannelClosed) => return Ok(()),
            Err(LogSendError::Abnormal(e)) => bail!(e),
        }
    } else {
        match stream_to_end(&mut lines, &mut sender).await {
            Ok(_) => (),
            Err(LogSendError::ChannelClosed) => return Ok(()),
            Err(LogSendError::Abnormal(e)) => bail!(e),
        }
    }

    if sender.follow() {
        loop {
            match stream_to_end(&mut lines, &mut sender).await {
                Ok(_) => (),
                Err(LogSendError::ChannelClosed) => return Ok(()),
                Err(LogSendError::Abnormal(e)) => bail!(e),
            }

            tokio::time::delay_for(std::time::Duration::from_millis(500)).await;
        }
    }

    sender.close();
    Ok(())
}
