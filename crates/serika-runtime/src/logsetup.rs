use std::fs::OpenOptions;

use crate::error::Result;
use crate::paths::AppPaths;

pub fn init(paths: &AppPaths, level: &str) -> Result<()> {
    paths.ensure()?;
    if paths.log_file.exists() {
        if let Ok(meta) = std::fs::metadata(&paths.log_file) {
            if meta.len() > 5_000_000 {
                let rotated = paths.log_file.with_extension("log.1");
                let _ = std::fs::rename(&paths.log_file, rotated);
            }
        }
    }
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&paths.log_file)?;
    let filter = tracing_subscriber::EnvFilter::try_new(level)
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_ansi(false)
        .with_writer(file)
        .try_init();
    Ok(())
}
