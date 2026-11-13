use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

use serde::Serialize;
use sm_core::{start_layers, ServiceConfig};
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatchAction {
    Start,
    Stop,
    Restart,
}

impl BatchAction {
    pub fn label(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Stop => "stop",
            Self::Restart => "restart",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatchState {
    Succeeded,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchEvent {
    pub action: String,
    pub name: String,
    pub state: String,
    pub message: Option<String>,
    pub done: usize,
    pub total: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchOutcome {
    pub name: String,
    pub state: BatchState,
    pub message: Option<String>,
}

pub trait BatchExecutor: Send + Sync {
    fn execute(
        &self,
        action: BatchAction,
        service: &ServiceConfig,
        cancel: &Arc<AtomicBool>,
    ) -> Result<(), String>;
}

pub async fn run_batch(
    action: BatchAction,
    services: Vec<ServiceConfig>,
    executor: Arc<dyn BatchExecutor>,
    cancel: Arc<AtomicBool>,
    max_parallel: usize,
    progress: Arc<dyn Fn(BatchEvent) + Send + Sync>,
) -> Vec<BatchOutcome> {
    let total = services.len();
    if total == 0 {
        return Vec::new();
    }

    let mut layers = start_layers(&services);
    if action == BatchAction::Stop {
        layers.reverse();
    }

    let semaphore = Arc::new(Semaphore::new(max_parallel.max(1)));
    let done = Arc::new(AtomicUsize::new(0));
    let mut outcomes = Vec::with_capacity(total);

    for layer in layers {
        let mut joins = JoinSet::new();

        for index in layer {
            let service = services[index].clone();
            let executor = executor.clone();
            let cancel = cancel.clone();
            let progress = progress.clone();
            let done = done.clone();
            let semaphore = semaphore.clone();

            joins.spawn(async move {
                let permit = semaphore
                    .acquire_owned()
                    .await
                    .expect("semaphore tidak pernah ditutup");
                let _permit = permit;
                let name = service.name.clone();

                if cancel.load(Ordering::SeqCst) {
                    let done_now = done.fetch_add(1, Ordering::SeqCst) + 1;
                    progress(BatchEvent {
                        action: action.label().to_string(),
                        name: name.clone(),
                        state: "cancelled".to_string(),
                        message: Some("Dibatalkan sebelum dijalankan.".to_string()),
                        done: done_now,
                        total,
                    });
                    return BatchOutcome {
                        name,
                        state: BatchState::Cancelled,
                        message: Some("Dibatalkan sebelum dijalankan.".to_string()),
                    };
                }

                let action_value = action;
                let cancel_value = cancel.clone();
                let result = tokio::task::spawn_blocking(move || {
                    executor.execute(action_value, &service, &cancel_value)
                })
                .await;

                let (state, message) = match result {
                    Ok(Ok(())) => (BatchState::Succeeded, None),
                    Ok(Err(message)) => (BatchState::Failed, Some(message)),
                    Err(error) => (BatchState::Failed, Some(error.to_string())),
                };

                let done_now = done.fetch_add(1, Ordering::SeqCst) + 1;
                let state_text = match state {
                    BatchState::Succeeded => "succeeded",
                    BatchState::Failed => "failed",
                    BatchState::Cancelled => "cancelled",
                };
                progress(BatchEvent {
                    action: action.label().to_string(),
                    name: name.clone(),
                    state: state_text.to_string(),
                    message: message.clone(),
                    done: done_now,
                    total,
                });

                BatchOutcome {
                    name,
                    state,
                    message,
                }
            });
        }

        while let Some(joined) = joins.join_next().await {
            if let Ok(outcome) = joined {
                outcomes.push(outcome);
            }
        }
    }

    outcomes
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::sync::Mutex;
    use std::time::Duration;

    #[derive(Default)]
    struct FakeExecutor {
        records: Mutex<Vec<(BatchAction, String)>>,
        active: AtomicUsize,
        max_active: AtomicUsize,
        delay_ms: u64,
        failures: Mutex<Vec<String>>,
    }

    impl FakeExecutor {
        fn with_delay(delay_ms: u64) -> Self {
            Self {
                delay_ms,
                ..Self::default()
            }
        }
    }

    impl BatchExecutor for FakeExecutor {
        fn execute(
            &self,
            action: BatchAction,
            service: &ServiceConfig,
            _cancel: &Arc<AtomicBool>,
        ) -> Result<(), String> {
            let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
            self.max_active.fetch_max(active, Ordering::SeqCst);
            self.records
                .lock()
                .unwrap()
                .push((action, service.name.clone()));
            if self.delay_ms > 0 {
                std::thread::sleep(Duration::from_millis(self.delay_ms));
            }
            self.active.fetch_sub(1, Ordering::SeqCst);

            if self
                .failures
                .lock()
                .unwrap()
                .iter()
                .any(|name| name == &service.name)
            {
                Err(format!("gagal: {}", service.name))
            } else {
                Ok(())
            }
        }
    }

    fn service(name: &str, dependencies: &[&str]) -> ServiceConfig {
        ServiceConfig {
            name: name.to_string(),
            dependencies: dependencies.iter().map(|value| value.to_string()).collect(),
            ..ServiceConfig::default()
        }
    }

    fn progress_sink(events: Arc<Mutex<Vec<BatchEvent>>>) -> Arc<dyn Fn(BatchEvent) + Send + Sync> {
        Arc::new(move |event| events.lock().unwrap().push(event))
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn start_batch_respects_dependency_layers() {
        let executor = Arc::new(FakeExecutor::default());
        let services = vec![service("app", &["db"]), service("db", &[])];
        let events = Arc::new(Mutex::new(Vec::new()));

        let outcomes = run_batch(
            BatchAction::Start,
            services,
            executor.clone(),
            Arc::new(AtomicBool::new(false)),
            4,
            progress_sink(events.clone()),
        )
        .await;

        assert_eq!(outcomes.len(), 2);
        assert!(outcomes
            .iter()
            .all(|outcome| outcome.state == BatchState::Succeeded));
        assert_eq!(executor.records.lock().unwrap()[0].1, "db");
        assert_eq!(executor.records.lock().unwrap()[1].1, "app");
        assert!(events.lock().unwrap().iter().all(|event| event.total == 2));
        let dones: Vec<usize> = events
            .lock()
            .unwrap()
            .iter()
            .map(|event| event.done)
            .collect();
        assert_eq!(dones, [1, 2]);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn stop_batch_reverses_layers() {
        let executor = Arc::new(FakeExecutor::default());
        let services = vec![service("app", &["db"]), service("db", &[])];

        run_batch(
            BatchAction::Stop,
            services,
            executor.clone(),
            Arc::new(AtomicBool::new(false)),
            4,
            Arc::new(|_| {}),
        )
        .await;

        let records = executor.records.lock().unwrap();
        assert_eq!(records[0].1, "app");
        assert_eq!(records[1].1, "db");
        assert!(records
            .iter()
            .all(|(action, _)| *action == BatchAction::Stop));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn independent_services_run_in_parallel() {
        let executor = Arc::new(FakeExecutor::with_delay(50));
        let services = vec![service("a", &[]), service("b", &[]), service("c", &[])];

        run_batch(
            BatchAction::Start,
            services,
            executor.clone(),
            Arc::new(AtomicBool::new(false)),
            3,
            Arc::new(|_| {}),
        )
        .await;

        assert!(executor.max_active.load(Ordering::SeqCst) >= 2);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn cancellation_prevents_execution() {
        let executor = Arc::new(FakeExecutor::default());
        let services = vec![service("a", &[]), service("b", &["a"])];
        let events = Arc::new(Mutex::new(Vec::new()));

        let outcomes = run_batch(
            BatchAction::Start,
            services,
            executor.clone(),
            Arc::new(AtomicBool::new(true)),
            2,
            progress_sink(events.clone()),
        )
        .await;

        assert!(executor.records.lock().unwrap().is_empty());
        assert!(outcomes
            .iter()
            .all(|outcome| outcome.state == BatchState::Cancelled));
        assert!(events
            .lock()
            .unwrap()
            .iter()
            .all(|event| event.state == "cancelled"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn failure_does_not_stop_other_services() {
        let executor = Arc::new(FakeExecutor {
            failures: Mutex::new(vec!["bad".into()]),
            ..FakeExecutor::default()
        });
        let services = vec![service("bad", &[]), service("good", &[])];

        let outcomes = run_batch(
            BatchAction::Start,
            services,
            executor,
            Arc::new(AtomicBool::new(false)),
            2,
            Arc::new(|_| {}),
        )
        .await;

        assert_eq!(outcomes.len(), 2);
        assert!(outcomes
            .iter()
            .any(|outcome| outcome.name == "bad" && outcome.state == BatchState::Failed));
        assert!(outcomes
            .iter()
            .any(|outcome| outcome.name == "good" && outcome.state == BatchState::Succeeded));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn empty_batch_is_noop() {
        let outcomes = run_batch(
            BatchAction::Start,
            Vec::new(),
            Arc::new(FakeExecutor::default()),
            Arc::new(AtomicBool::new(false)),
            4,
            Arc::new(|_| {}),
        )
        .await;
        assert!(outcomes.is_empty());
    }
}
