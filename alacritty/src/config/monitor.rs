use std::path::PathBuf;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use log::{debug, error, warn};
use notify::event::{ModifyKind, RenameMode};
use notify::{
    Config, Error as NotifyError, Event as NotifyEvent, EventKind, RecommendedWatcher,
    RecursiveMode, Watcher,
};
use winit::event_loop::EventLoopProxy;

use alacritty_terminal::thread;

use crate::event::{Event, EventType};

const DEBOUNCE_DELAY: Duration = Duration::from_millis(10);

/// The fallback for `RecommendedWatcher` polling.
const FALLBACK_POLLING_TIMEOUT: Duration = Duration::from_secs(1);

/// Config file update monitor.
pub struct ConfigMonitor {
    thread: JoinHandle<()>,
    shutdown_tx: Sender<Result<NotifyEvent, NotifyError>>,
    watched_paths: Vec<PathBuf>,
}

impl ConfigMonitor {
    pub fn new(mut paths: Vec<PathBuf>, event_proxy: EventLoopProxy<Event>) -> Option<Self> {
        // Don't monitor config if there is no path to watch.
        if paths.is_empty() {
            return None;
        }

        // Preserve the logical import set before expanding it for filesystem watching.
        let watched_paths = Self::normalize_paths(&paths);

        // Exclude char devices like `/dev/null`, sockets, and so on, by checking that file type is
        // a regular file.
        paths.retain(|path| {
            // Call `metadata` to resolve symbolic links.
            path.metadata().is_ok_and(|metadata| metadata.file_type().is_file())
        });

        // Canonicalize paths, keeping the base paths for symlinks.
        for i in 0..paths.len() {
            if let Ok(canonical_path) = paths[i].canonicalize() {
                match paths[i].symlink_metadata() {
                    Ok(metadata) if metadata.file_type().is_symlink() => paths.push(canonical_path),
                    _ => paths[i] = canonical_path,
                }
            }
        }

        // The Duration argument is a debouncing period.
        let (tx, rx) = mpsc::channel();
        let mut watcher = match RecommendedWatcher::new(
            tx.clone(),
            Config::default().with_poll_interval(FALLBACK_POLLING_TIMEOUT),
        ) {
            Ok(watcher) => watcher,
            Err(err) => {
                error!("Unable to watch config file: {err}");
                return None;
            },
        };

        let join_handle = thread::spawn_named("config watcher", move || {
            // Get all unique parent directories.
            let mut parents = paths
                .iter()
                .map(|path| {
                    let mut path = path.clone();
                    let _ = path.pop();
                    path
                })
                .collect::<Vec<PathBuf>>();
            parents.sort_unstable();
            parents.dedup();

            // Watch all configuration file directories.
            for parent in &parents {
                if let Err(err) = watcher.watch(parent, RecursiveMode::NonRecursive) {
                    debug!("Unable to watch config directory {}: {err}", parent.display());
                }
            }

            // The current debouncing time.
            let mut debouncing_deadline: Option<Instant> = None;

            // The events accumulated during the debounce period.
            let mut received_events = Vec::new();

            loop {
                // We use `recv_timeout` to debounce the events coming from the watcher and reduce
                // the amount of config reloads.
                let event = if let Some(debouncing_deadline) = debouncing_deadline.as_ref() {
                    rx.recv_timeout(debouncing_deadline.saturating_duration_since(Instant::now()))
                } else {
                    let event = rx.recv().map_err(Into::into);
                    // Set the debouncing deadline after receiving the event.
                    debouncing_deadline = Some(Instant::now() + DEBOUNCE_DELAY);
                    event
                };

                match event {
                    Ok(Ok(event)) => match event.kind {
                        EventKind::Other if event.info() == Some("shutdown") => break,
                        // Ignore when config file is moved as it's equivalent to deletion.
                        // Some editors trigger this as they move the file as part of saving.
                        EventKind::Modify(ModifyKind::Name(
                            RenameMode::From | RenameMode::Both,
                        )) => (),
                        EventKind::Any
                        | EventKind::Create(_)
                        | EventKind::Modify(_)
                        | EventKind::Other => {
                            received_events.push(event);
                        },
                        _ => (),
                    },
                    Err(RecvTimeoutError::Timeout) => {
                        // Go back to polling the events.
                        debouncing_deadline = None;

                        if received_events
                            .drain(..)
                            .flat_map(|event| event.paths.into_iter())
                            .any(|path| paths.contains(&path))
                        {
                            // Always reload the primary configuration file.
                            let event = Event::new(EventType::ConfigReload(paths[0].clone()), None);
                            let _ = event_proxy.send_event(event);
                        }
                    },
                    Ok(Err(err)) => {
                        debug!("Config watcher errors: {err:?}");
                    },
                    Err(err) => {
                        debug!("Config watcher channel dropped unexpectedly: {err}");
                        break;
                    },
                }
            }
        });

        Some(Self { watched_paths, thread: join_handle, shutdown_tx: tx })
    }

    /// Synchronously shut down the monitor.
    pub fn shutdown(self) {
        // Request shutdown.
        let mut event = NotifyEvent::new(EventKind::Other);
        event = event.set_info("shutdown");
        let _ = self.shutdown_tx.send(Ok(event));

        // Wait for thread to terminate.
        if let Err(err) = self.thread.join() {
            warn!("config monitor shutdown failed: {err:?}");
        }
    }

    /// Check if the config monitor needs to be restarted.
    ///
    /// This checks the supplied list of files against the monitored files to determine if a
    /// restart is necessary.
    pub fn needs_restart(&self, files: &[PathBuf]) -> bool {
        Self::normalize_paths(files) != self.watched_paths
    }

    fn normalize_paths(files: &[PathBuf]) -> Vec<PathBuf> {
        let mut files = files.to_vec();
        files.sort_unstable();
        files.dedup();
        files
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn monitor(paths: &[&str]) -> ConfigMonitor {
        let (shutdown_tx, _shutdown_rx) = mpsc::channel();
        ConfigMonitor {
            thread: std::thread::spawn(|| {}),
            shutdown_tx,
            watched_paths: ConfigMonitor::normalize_paths(
                &paths.iter().map(PathBuf::from).collect::<Vec<_>>(),
            ),
        }
    }

    #[test]
    fn restart_only_when_import_set_changes() {
        let monitor = monitor(&["primary.toml", "import.toml"]);

        assert!(!monitor.needs_restart(&[
            PathBuf::from("import.toml"),
            PathBuf::from("primary.toml"),
            PathBuf::from("import.toml"),
        ]));
        assert!(monitor.needs_restart(&[PathBuf::from("primary.toml")]));
    }
}
