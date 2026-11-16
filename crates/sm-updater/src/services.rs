use std::cell::RefCell;
use std::collections::BTreeSet;
use std::time::Duration;

use crate::registry::ServicePlan;

/// Abstraksi operasi Windows Service Control Manager agar bisa diuji.
pub trait ServiceControl: Send + Sync {
    fn exists(&self, name: &str) -> Result<bool, String>;
    fn is_running(&self, name: &str) -> Result<bool, String>;
    fn stop(&self, name: &str, timeout: Duration) -> Result<(), String>;
    fn start(&self, name: &str, timeout: Duration) -> Result<(), String>;
    fn restart(&self, name: &str, timeout: Duration) -> Result<(), String>;

    /// Preflight hak akses; default tidak melakukan apa-apa untuk fake/test.
    fn preflight(&self, _name: &str) -> Result<(), String> {
        Ok(())
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct WindowsServiceControl;

impl ServiceControl for WindowsServiceControl {
    fn exists(&self, name: &str) -> Result<bool, String> {
        sm_win::service_exists(name).map_err(|error| error.to_string())
    }

    fn is_running(&self, name: &str) -> Result<bool, String> {
        sm_win::query_status(name)
            .map(|status| status.state.is_running())
            .map_err(|error| error.to_string())
    }

    fn stop(&self, name: &str, timeout: Duration) -> Result<(), String> {
        sm_win::stop_service(name, &sm_win::WaitOptions::with_timeout(timeout))
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    fn start(&self, name: &str, timeout: Duration) -> Result<(), String> {
        sm_win::start_service(name, &sm_win::WaitOptions::with_timeout(timeout))
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    fn restart(&self, name: &str, timeout: Duration) -> Result<(), String> {
        sm_win::restart_service(name, &sm_win::WaitOptions::with_timeout(timeout))
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    fn preflight(&self, name: &str) -> Result<(), String> {
        sm_win::check_control_access(name).map_err(|error| {
            format!(
                "Tidak punya hak start/stop service '{name}' (jalankan sebagai Administrator/elevated): {error}"
            )
        })
    }
}

/// Mencatat service yang dihentikan oleh updater agar bisa dipulihkan.
pub struct Lifecycle<'a> {
    control: &'a dyn ServiceControl,
    timeout: Duration,
    stopped: RefCell<Vec<String>>,
}

impl<'a> Lifecycle<'a> {
    pub fn new(control: &'a dyn ServiceControl, timeout_seconds: u64) -> Self {
        Self {
            control,
            timeout: Duration::from_secs(timeout_seconds.max(1)),
            stopped: RefCell::new(Vec::new()),
        }
    }

    pub fn validate(&self, plan: &ServicePlan) -> Result<(), String> {
        for name in plan_names(plan) {
            self.validate_name(&name)?;
            self.control.preflight(&name)?;
        }
        Ok(())
    }

    /// Validasi nama service tanpa memeriksa hak start/stop (untuk dry-run).
    pub fn validate_readonly(&self, plan: &ServicePlan) -> Result<(), String> {
        for name in plan_names(plan) {
            self.validate_name(&name)?;
        }
        Ok(())
    }

    fn validate_name(&self, name: &str) -> Result<(), String> {
        if !self.control.exists(name)? {
            return Err(format!(
                "Service '{name}' tidak ditemukan di Windows SCM. Periksa data/services.json."
            ));
        }
        Ok(())
    }

    pub fn stop_before_pull(&self, plan: &ServicePlan) -> Result<(), String> {
        self.stop_list(&plan.stop_before_pull)
    }

    pub fn stop_before_build(&self, plan: &ServicePlan) -> Result<(), String> {
        self.stop_list(&plan.stop_before_build)
    }

    /// Start kembali service yang dihentikan updater, lalu restart/start sesuai plan.
    pub fn finish(&self, plan: &ServicePlan) -> Result<(), String> {
        let mut errors = Vec::new();
        let mut handled = BTreeSet::new();

        let stopped = std::mem::take(&mut *self.stopped.borrow_mut());
        for name in stopped {
            if !handled.insert(name.clone()) {
                continue;
            }
            if let Err(error) = self.control.start(&name, self.timeout) {
                errors.push(format!("start '{name}' gagal: {error}"));
            }
        }

        for name in unique_names(&plan.restart_after) {
            if !handled.insert(name.clone()) {
                continue;
            }
            if let Err(error) = self.control.restart(&name, self.timeout) {
                errors.push(format!("restart '{name}' gagal: {error}"));
            }
        }

        for name in unique_names(&plan.start_after) {
            if !handled.insert(name.clone()) {
                continue;
            }
            if let Err(error) = self.control.start(&name, self.timeout) {
                errors.push(format!("start '{name}' gagal: {error}"));
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    }

    /// Best-effort saat update gagal setelah service dihentikan.
    pub fn best_effort_restore(&self) -> Vec<String> {
        let stopped = std::mem::take(&mut *self.stopped.borrow_mut());
        let mut errors = Vec::new();
        for name in stopped {
            if let Err(error) = self.control.start(&name, self.timeout) {
                errors.push(format!("start '{name}' gagal: {error}"));
            }
        }
        errors
    }

    fn stop_list(&self, names: &[String]) -> Result<(), String> {
        let mut result = Ok(());
        for name in unique_names(names) {
            if self.stopped.borrow().iter().any(|stopped| stopped == &name) {
                continue;
            }
            match self.control.is_running(&name) {
                Ok(false) => continue,
                Ok(true) => {}
                Err(error) => {
                    result = Err(format!("Gagal memeriksa service '{name}': {error}"));
                    break;
                }
            }
            match self.control.stop(&name, self.timeout) {
                Ok(()) => self.stopped.borrow_mut().push(name),
                Err(error) => {
                    result = Err(format!("Gagal menghentikan service '{name}': {error}"));
                    break;
                }
            }
        }

        if result.is_err() {
            let errors = self.best_effort_restore();
            if !errors.is_empty() {
                if let Err(message) = &mut result {
                    message.push_str(&format!("; pemulihan service: {}", errors.join("; ")));
                }
            }
        }
        result
    }
}

fn plan_names(plan: &ServicePlan) -> Vec<String> {
    let mut names = Vec::new();
    for list in [
        &plan.stop_before_pull,
        &plan.stop_before_build,
        &plan.restart_after,
        &plan.start_after,
    ] {
        for name in unique_names(list) {
            if !names.contains(&name) {
                names.push(name);
            }
        }
    }
    names
}

fn unique_names(values: &[String]) -> Vec<String> {
    let mut names = Vec::new();
    for value in values {
        let value = value.trim().to_string();
        if !value.is_empty() && !names.contains(&value) {
            names.push(value);
        }
    }
    names
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    #[derive(Default)]
    struct FakeControl {
        known: BTreeSet<String>,
        running: Mutex<BTreeSet<String>>,
        calls: Mutex<Vec<String>>,
        fail_stop: Option<String>,
    }

    impl FakeControl {
        fn new(known: &[&str], running: &[&str]) -> Self {
            Self {
                known: known.iter().map(|value| value.to_string()).collect(),
                running: Mutex::new(running.iter().map(|value| value.to_string()).collect()),
                ..Self::default()
            }
        }

        fn calls(&self) -> Vec<String> {
            self.calls.lock().unwrap().clone()
        }

        fn call_count(&self) -> BTreeMap<String, usize> {
            let mut counts = BTreeMap::new();
            for call in self.calls() {
                *counts.entry(call).or_insert(0) += 1;
            }
            counts
        }
    }

    impl ServiceControl for FakeControl {
        fn exists(&self, name: &str) -> Result<bool, String> {
            Ok(self.known.contains(name))
        }

        fn is_running(&self, name: &str) -> Result<bool, String> {
            Ok(self.running.lock().unwrap().contains(name))
        }

        fn stop(&self, name: &str, _timeout: Duration) -> Result<(), String> {
            self.calls.lock().unwrap().push(format!("stop:{name}"));
            if self.fail_stop.as_deref() == Some(name) {
                return Err("access denied".to_string());
            }
            self.running.lock().unwrap().remove(name);
            Ok(())
        }

        fn start(&self, name: &str, _timeout: Duration) -> Result<(), String> {
            self.calls.lock().unwrap().push(format!("start:{name}"));
            self.running.lock().unwrap().insert(name.to_string());
            Ok(())
        }

        fn restart(&self, name: &str, _timeout: Duration) -> Result<(), String> {
            self.calls.lock().unwrap().push(format!("restart:{name}"));
            self.running.lock().unwrap().insert(name.to_string());
            Ok(())
        }
    }

    fn plan() -> ServicePlan {
        ServicePlan {
            stop_before_build: vec!["api".into()],
            restart_after: vec!["api".into()],
            wait_timeout_seconds: 5,
            ..ServicePlan::default()
        }
    }

    #[test]
    fn stops_running_service_and_starts_it_after_update() {
        let control = FakeControl::new(&["api"], &["api"]);
        let lifecycle = Lifecycle::new(&control, 5);
        lifecycle.validate(&plan()).unwrap();
        lifecycle.stop_before_build(&plan()).unwrap();
        lifecycle.finish(&plan()).unwrap();
        assert_eq!(control.calls(), ["stop:api", "start:api"]);
    }

    #[test]
    fn already_stopped_service_stays_stopped() {
        let control = FakeControl::new(&["api"], &[]);
        let lifecycle = Lifecycle::new(&control, 5);
        let plan = ServicePlan {
            stop_before_build: vec!["api".into()],
            ..ServicePlan::default()
        };
        lifecycle.stop_before_build(&plan).unwrap();
        lifecycle.finish(&plan).unwrap();
        assert!(control.calls().is_empty());
    }

    #[test]
    fn restarts_running_service_listed_only_in_restart_after() {
        let control = FakeControl::new(&["api"], &["api"]);
        let lifecycle = Lifecycle::new(&control, 5);
        let plan = ServicePlan {
            restart_after: vec!["api".into()],
            ..ServicePlan::default()
        };
        lifecycle.finish(&plan).unwrap();
        assert_eq!(control.calls(), ["restart:api"]);
    }

    #[test]
    fn stop_failure_restores_services_stopped_earlier() {
        let mut control = FakeControl::new(&["a", "b"], &["a", "b"]);
        control.fail_stop = Some("b".to_string());
        let lifecycle = Lifecycle::new(&control, 5);
        let plan = ServicePlan {
            stop_before_build: vec!["a".into(), "b".into()],
            ..ServicePlan::default()
        };
        let error = lifecycle.stop_before_build(&plan).unwrap_err();
        assert!(error.contains("b"));
        let counts = control.call_count();
        assert_eq!(counts.get("stop:a"), Some(&1));
        assert_eq!(counts.get("stop:b"), Some(&1));
        assert_eq!(counts.get("start:a"), Some(&1));
    }

    #[test]
    fn validate_rejects_unknown_service() {
        let control = FakeControl::new(&["api"], &[]);
        let lifecycle = Lifecycle::new(&control, 5);
        let plan = ServicePlan {
            restart_after: vec!["missing".into()],
            ..ServicePlan::default()
        };
        let error = lifecycle.validate(&plan).unwrap_err();
        assert!(error.contains("missing"));
    }
}
