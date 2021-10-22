//! A simple abstraction layer over OS specific details on watching a filesystem. Due to a bug in
//! MacOS with sending an event on socket creation, we need to implement our own hacky watcher. To
//! keep it as clean as possible, this module abstracts those details away behind a `Stream`
//! implementation. A bug has been filed with Apple and we can remove this if/when the bug is fixed.
//! The bug ID is FB8830541 and @thomastaylor312 can check the status of it

use std::{
    path::Path,
    pin::Pin,
    task::{Context, Poll},
};

use futures::Stream;
#[cfg(not(target_os = "macos"))]
use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher};
use notify::{Event, Result as NotifyResult};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver};
use tracing::error;

pub struct FileSystemWatcher {
    recv: UnboundedReceiver<NotifyResult<Event>>,
    #[cfg(not(target_os = "macos"))]
    _watcher: RecommendedWatcher, // holds on to the watcher so it doesn't get dropped
}

impl Stream for FileSystemWatcher {
    type Item = NotifyResult<Event>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.recv.poll_recv(cx)
    }
}

// For Windows and Linux, just use notify. For Mac, use our hacky workaround
impl FileSystemWatcher {
    #[cfg(not(target_os = "macos"))]
    pub fn new<P: AsRef<Path>>(path: P) -> anyhow::Result<Self> {
        let (stream_tx, stream_rx) = unbounded_channel::<NotifyResult<Event>>();
        let mut watcher: RecommendedWatcher = notify::recommended_watcher(move |res| {
            if let Err(e) = stream_tx.send(res) {
                error!(error = %e, "Unable to send inotify event into stream")
            }
        })?;
        watcher.configure(Config::PreciseEvents(true))?;

        watcher.watch(path.as_ref(), RecursiveMode::NonRecursive)?;

        Ok(FileSystemWatcher {
            recv: stream_rx,
            _watcher: watcher,
        })
    }

    #[cfg(target_os = "macos")]
    pub fn new<P: AsRef<Path>>(path: P) -> anyhow::Result<Self> {
        Ok(FileSystemWatcher {
            recv: mac::dir_watcher(path),
        })
    }
}

#[cfg(target_os = "macos")]
mod mac {
    use std::collections::HashSet;
    use std::path::PathBuf;

    use super::*;
    use notify::event::{CreateKind, EventKind, RemoveKind};
    use notify::Error as NotifyError;
    use tokio::fs::DirEntry;
    use tokio::sync::mpsc::UnboundedSender;
    use tokio::time::{self, Duration};
    use tokio_stream::wrappers::ReadDirStream;
    use tokio_stream::StreamExt;

    const WAIT_TIME: u64 = 2;

    pub fn dir_watcher<P: AsRef<Path>>(dir: P) -> UnboundedReceiver<NotifyResult<Event>> {
        let (tx, rx) = unbounded_channel();
        let path = dir.as_ref().to_path_buf();
        tokio::spawn(async move {
            let mut path_cache: HashSet<PathBuf> = match get_dir_list(&path).await {
                Ok(set) => set,
                Err(e) => {
                    error!(
                        error = %e,
                        path = %path.display(),
                        "Unable to refresh directory, will attempt again"
                    );
                    HashSet::new()
                }
            };

            loop {
                let current_paths: HashSet<PathBuf> = match get_dir_list(&path).await {
                    Ok(set) => set,
                    Err(e) => {
                        error!(
                            error = %e,
                            path = %path.display(),
                            "Unable to refresh directory, will attempt again"
                        );
                        if let Err(e) = tx.send(Err(NotifyError::io(e))) {
                            error!(result = ?e.0, "Unable to send error due to channel being closed");
                        }
                        continue;
                    }
                };

                // Do a difference between cached and current paths (current - cached) to detect set of creates
                send_creates(tx.clone(), current_paths.difference(&path_cache).cloned());

                // Do a difference between cached and current paths (cached - current) to detect set of deletes
                send_deletes(tx.clone(), path_cache.difference(&current_paths).cloned());

                // Now we can set current to cached
                path_cache = current_paths;

                time::sleep(Duration::from_secs(WAIT_TIME)).await;
            }
        });
        rx
    }

    async fn get_dir_list(path: &Path) -> Result<HashSet<PathBuf>, std::io::Error> {
        // What does this monstrosity do? Well, due to async and all the random streaming involved
        // this:
        // 1. Reads the directory as a stream
        // 2. Maps the stream to a Vec of entries and handles any errors
        // 3. Converts the entries to PathBufs and puts them in a HashSet
        ReadDirStream::new(tokio::fs::read_dir(path).await?)
            .collect::<Result<Vec<DirEntry>, _>>()
            .await
            .map(|entries| {
                entries
                    .into_iter()
                    .map(|e| e.path())
                    .collect::<HashSet<PathBuf>>()
            })
    }

    fn send_creates(
        tx: UnboundedSender<NotifyResult<Event>>,
        items: impl Iterator<Item = PathBuf>,
    ) {
        send_event_with_kind(tx, items, EventKind::Create(CreateKind::Any))
    }

    fn send_deletes(
        tx: UnboundedSender<NotifyResult<Event>>,
        items: impl Iterator<Item = PathBuf>,
    ) {
        send_event_with_kind(tx, items, EventKind::Remove(RemoveKind::Any))
    }

    fn send_event_with_kind(
        tx: UnboundedSender<NotifyResult<Event>>,
        items: impl Iterator<Item = PathBuf>,
        kind: EventKind,
    ) {
        let paths: Vec<PathBuf> = items.collect();
        // If there were no paths, it means there weren't any files deleted, so return
        if paths.is_empty() {
            return;
        }
        let event = Event {
            kind,
            paths,
            ..Default::default()
        };
        if let Err(e) = tx.send(Ok(event)) {
            // At this point there isn't much we can do as the channel is closed. So just log an
            // error
            error!(
                result = ?e.0,
                "Unable to send event due to the channel being closed"
            );
        }
    }

    #[cfg(test)]
    mod test {
        use super::*;

        #[tokio::test]
        async fn test_send_deletes() {
            let (tx, mut rx) = unbounded_channel();
            let file1 = PathBuf::from("/foo/bar");
            let file2 = PathBuf::from("/bar/foo");

            send_deletes(tx, vec![file1.clone(), file2.clone()].into_iter());
            let event = rx
                .recv()
                .await
                .expect("got None result, which means the channel was closed prematurely")
                .expect("Got error from watch");

            assert!(event.kind.is_remove(), "Event is not a delete type");
            assert!(event.paths.len() == 2, "Event should contain two paths");
            assert!(event.paths.contains(&file1), "Missing expected path");
            assert!(event.paths.contains(&file2), "Missing expected path");
        }

        #[tokio::test]
        async fn test_send_creates() {
            let (tx, mut rx) = unbounded_channel();
            let file1 = PathBuf::from("/foo/bar");
            let file2 = PathBuf::from("/bar/foo");

            send_creates(tx, vec![file1.clone(), file2.clone()].into_iter());
            let event = rx
                .recv()
                .await
                .expect("got None result, which means the channel was closed prematurely")
                .expect("Got error from watch");

            assert!(event.kind.is_create(), "Event is not a create type");
            assert!(event.paths.len() == 2, "Event should contain two paths");
            assert!(event.paths.contains(&file1), "Missing expected path");
            assert!(event.paths.contains(&file2), "Missing expected path");
        }

        #[tokio::test]
        async fn test_watcher() {
            let temp = tempfile::tempdir().expect("unable to set up temporary directory");

            // Create some "existing" files in the directory
            let first = tokio::fs::write(temp.path().join("old_foo.txt"), "");
            let second = tokio::fs::write(temp.path().join("old_bar.txt"), "");

            tokio::try_join!(first, second).expect("unable to write test files");

            let mut rx = dir_watcher(&temp);

            let base = temp.path().to_owned();
            tokio::spawn(create_files(base));

            let event = tokio::time::timeout(Duration::from_secs(WAIT_TIME + 1), rx.recv())
                .await
                .expect("Timed out waiting for event")
                .expect("got None result, which means the channel was closed prematurely")
                .expect("Got error from watch");

            let mut found_create = false;
            let mut found_delete = false;

            assert_event(event, &temp, &mut found_create, &mut found_delete);

            let event = tokio::time::timeout(Duration::from_secs(WAIT_TIME + 1), rx.recv())
                .await
                .expect("Timed out waiting for event")
                .expect("got None result, which means the channel was closed prematurely")
                .expect("Got error from watch");

            assert_event(event, &temp, &mut found_create, &mut found_delete);

            // We should only get two different events, so this is just waiting for 1 second longer
            // than the loop to make sure we don't get another event
            assert!(
                tokio::time::timeout(Duration::from_secs(WAIT_TIME + 1), rx.recv())
                    .await
                    .is_err(),
                "Should not have gotten another event"
            );
        }

        async fn create_files(base: PathBuf) {
            // Wait for a bit to make sure things are started
            tokio::time::sleep(Duration::from_secs(1)).await;
            let first = tokio::fs::write(base.join("new_foo.txt"), "");
            let second = tokio::fs::write(base.join("new_bar.txt"), "");
            let third = tokio::fs::remove_file(base.join("old_foo.txt"));

            tokio::try_join!(first, second, third).expect("unable to write/delete test files");
        }

        fn assert_event(
            event: Event,
            base: impl AsRef<Path>,
            found_create: &mut bool,
            found_delete: &mut bool,
        ) {
            match event.kind {
                EventKind::Create(_) => {
                    // Check if we already got a create event
                    if *found_create {
                        panic!("Got second create event");
                    }
                    assert!(event.paths.len() == 2, "Expected two created paths");
                    assert!(event.paths.contains(&base.as_ref().join("new_foo.txt")));
                    assert!(event.paths.contains(&base.as_ref().join("new_bar.txt")));
                    *found_create = true;
                }
                EventKind::Remove(_) => {
                    // Check if we already got a delete event
                    if *found_delete {
                        panic!("Got second delete event");
                    }
                    assert!(event.paths.len() == 1, "Expected 1 deleted path");
                    assert!(event.paths.contains(&base.as_ref().join("old_foo.txt")));
                    *found_delete = true;
                }
                _ => panic!("Event wasn't a create or remove"),
            }
        }
    }
}
