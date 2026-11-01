use crate::config::AppConfig;
use anyhow::Result;
use notify::{Event, EventKind, RecursiveMode, Watcher};
use std::path::Path;
use tokio::sync::broadcast;
use tracing::{error, info, warn};

pub const CONFIG_DIR: &str = "config";
pub const CONFIG_PATH: &str = "config/app_config.conf";

/// Watches app_config.conf and broadcasts a new AppConfig whenever it changes.
/// The directory is watched rather than the file, so changes survive editors
/// and atomic saves that replace the file with a new one.
/// Does not stop on parse errors — keeps the last valid config active.
pub async fn watch_config(tx: broadcast::Sender<AppConfig>) -> Result<()> {
    let (notify_tx, mut notify_rx) = tokio::sync::mpsc::channel::<Event>(32);

    let mut watcher = notify::recommended_watcher(move |res: notify::Result<Event>| {
        match res {
            Ok(event) => {
                let _ = notify_tx.blocking_send(event);
            }
            Err(e) => error!("File watcher error: {}", e),
        }
    })?;

    watcher.watch(Path::new(CONFIG_DIR), RecursiveMode::NonRecursive)?;
    info!("Watching {} for changes", CONFIG_PATH);

    let config_file = Path::new(CONFIG_PATH).file_name();
    while let Some(event) = notify_rx.recv().await {
        if !event.paths.iter().any(|p| p.file_name() == config_file) {
            continue;
        }
        match event.kind {
            EventKind::Modify(_) | EventKind::Create(_) => {
                // Small debounce: editors often write twice
                tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
                match AppConfig::load(CONFIG_PATH) {
                    Ok(cfg) => {
                        info!("Config reloaded from {}", CONFIG_PATH);
                        let _ = tx.send(cfg);
                    }
                    Err(e) => {
                        warn!("Config reload failed (keeping previous): {}", e);
                    }
                }
            }
            _ => {}
        }
    }

    Ok(())
}
