use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

#[derive(Debug)]
pub struct UpdateLock {
    path: PathBuf,
}

impl UpdateLock {
    pub fn acquire(path: &Path, stale_after: Duration) -> Result<Self, String> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }

        if path.exists() {
            let stale = fs::metadata(path)
                .and_then(|metadata| metadata.modified())
                .map(|modified| {
                    SystemTime::now()
                        .duration_since(modified)
                        .unwrap_or_default()
                        > stale_after
                })
                .unwrap_or(false);
            if stale {
                let _ = fs::remove_file(path);
            } else {
                let content = fs::read_to_string(path).unwrap_or_default();
                return Err(format!(
                    "Update lain sedang berjalan (lock: {}). {}",
                    path.display(),
                    content.trim()
                ));
            }
        }

        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|error| {
                format!(
                    "Tidak bisa membuat lock {}: {error}. Mungkin update lain sedang berjalan.",
                    path.display()
                )
            })?;
        let line = format!(
            "pid={} started={}\n",
            std::process::id(),
            chrono::Local::now().to_rfc3339()
        );
        file.write_all(line.as_bytes())
            .map_err(|error| error.to_string())?;

        Ok(Self {
            path: path.to_path_buf(),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for UpdateLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lock_prevents_second_acquire_and_releases() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("data/updater.lock");
        let lock = UpdateLock::acquire(&path, Duration::from_secs(60)).unwrap();
        assert!(path.is_file());
        let error = UpdateLock::acquire(&path, Duration::from_secs(60)).unwrap_err();
        assert!(error.contains("sedang berjalan"));
        drop(lock);
        assert!(!path.exists());
    }

    #[test]
    fn stale_lock_can_be_replaced() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("updater.lock");
        fs::create_dir_all(temp.path()).unwrap();
        fs::write(&path, b"pid=old\n").unwrap();
        let lock = UpdateLock::acquire(&path, Duration::from_secs(0)).unwrap();
        assert!(lock.path().is_file());
    }
}
