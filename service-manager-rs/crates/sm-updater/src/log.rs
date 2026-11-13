use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

pub struct UpdateLogger {
    run_log: PathBuf,
    project_log: Option<PathBuf>,
}

impl UpdateLogger {
    pub fn new(log_dir: &Path, project_path: &Path, stamp: &str) -> Self {
        let _ = fs::create_dir_all(log_dir);
        let run_log = log_dir.join(format!("project-update-{stamp}.log"));

        let project_log_dir = project_path.join("storage").join("logs");
        let project_log = if fs::create_dir_all(&project_log_dir).is_ok() {
            Some(project_log_dir.join(format!("project-update-{stamp}.log")))
        } else {
            None
        };

        Self {
            run_log,
            project_log,
        }
    }

    pub fn log(&self, message: &str) {
        let line = format!(
            "{} {}\r\n",
            chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
            message
        );
        print!("{line}");
        let _ = append(&self.run_log, &line);
        if let Some(path) = &self.project_log {
            let _ = append(path, &line);
        }
    }

    pub fn run_log_path(&self) -> &Path {
        &self.run_log
    }

    pub fn project_log_path(&self) -> Option<&Path> {
        self.project_log.as_deref()
    }
}

fn append(path: &Path, line: &str) -> std::io::Result<()> {
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    file.write_all(line.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_run_and_project_logs() {
        let temp = tempfile::tempdir().unwrap();
        let project = temp.path().join("project");
        fs::create_dir_all(&project).unwrap();
        let logger = UpdateLogger::new(&temp.path().join("logs"), &project, "20260101-000000");
        logger.log("hello");

        let run_log = logger.run_log_path();
        let project_log = logger.project_log_path().unwrap();
        assert!(run_log.is_file());
        assert!(project_log.is_file());
        assert!(fs::read_to_string(run_log).unwrap().contains("hello"));
        assert!(fs::read_to_string(project_log).unwrap().contains("hello"));
    }
}
