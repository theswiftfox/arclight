//! Bound blocking work independently of the UI and Tokio's async workers.
use std::{
    future::Future,
    sync::{Arc, LazyLock},
    time::Duration,
};

use tokio::sync::Semaphore;

pub const IO_TIMEOUT: Duration = Duration::from_secs(10);
pub const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(30);
pub const SETUP_TIMEOUT: Duration = Duration::from_mins(5);

/// Tokio's default `Runtime::drop` waits for every blocking job. A stuck mount
/// must not keep the UI thread stuck during application shutdown either.
pub struct Executor(Option<tokio::runtime::Runtime>);

impl iced::Executor for Executor {
    fn new() -> std::io::Result<Self> {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .map(|runtime| Self(Some(runtime)))
    }

    fn spawn(&self, future: impl Future<Output = ()> + Send + 'static) {
        self.0.as_ref().expect("runtime").spawn(future);
    }

    fn block_on<T>(&self, future: impl Future<Output = T>) -> T {
        self.0.as_ref().expect("runtime").block_on(future)
    }

    fn enter<R>(&self, f: impl FnOnce() -> R) -> R {
        let _guard = self.0.as_ref().expect("runtime").enter();
        f()
    }
}

impl Drop for Executor {
    fn drop(&mut self) {
        if let Some(runtime) = self.0.take() {
            runtime.shutdown_background();
        }
    }
}

static WORKERS: LazyLock<Arc<Semaphore>> = LazyLock::new(|| Arc::new(Semaphore::new(4)));
// A timed-out filesystem mutation cannot be cancelled. Keep this permit inside
// the blocking closure until it really returns, so a retry cannot race it.
static MUTATIONS: LazyLock<Arc<Semaphore>> = LazyLock::new(|| Arc::new(Semaphore::new(1)));
static PREFIX_SETUP: LazyLock<Arc<Semaphore>> = LazyLock::new(|| Arc::new(Semaphore::new(1)));

/// Acquire exclusive access to prefix setup operations.
///
/// # Errors
/// Returns an error if another prefix setup operation is still running.
pub fn prefix_setup() -> anyhow::Result<tokio::sync::OwnedSemaphorePermit> {
    PREFIX_SETUP.clone().try_acquire_owned().map_err(|_| {
        anyhow::anyhow!("Another launch, installation, or settings save is still running")
    })
}

struct AbortOnDrop<T>(tokio::task::JoinHandle<T>);

impl<T> Drop for AbortOnDrop<T> {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// Run asynchronous work with a deadline.
///
/// # Errors
/// Returns the work's error, or an error if the deadline expires.
pub async fn deadline<T>(
    name: &'static str,
    duration: Duration,
    work: impl Future<Output = anyhow::Result<T>>,
) -> anyhow::Result<T> {
    tokio::time::timeout(duration, work)
        .await
        .map_err(|_| anyhow::anyhow!("{name} timed out after {} seconds", duration.as_secs()))?
}

/// Run blocking work on a bounded worker pool with a deadline.
///
/// # Errors
/// Returns an error if all workers are busy, the worker fails, the deadline
/// expires, or the work itself fails.
pub async fn blocking<T: Send + 'static>(
    name: &'static str,
    duration: Duration,
    work: impl FnOnce() -> anyhow::Result<T> + Send + 'static,
) -> anyhow::Result<T> {
    // Do not queue unbounded work behind stalled filesystem calls.
    let permit = WORKERS
        .clone()
        .try_acquire_owned()
        .map_err(|_| anyhow::anyhow!("{name}: background workers are busy; try again later"))?;
    let mut job = AbortOnDrop(tokio::task::spawn_blocking(move || {
        let _permit = permit;
        work()
    }));
    let result = deadline(name, duration, async {
        (&mut job.0)
            .await
            .map_err(|e| anyhow::anyhow!("{name} worker failed: {e}"))?
    })
    .await;
    // Cancels queued jobs; already-running jobs retain their permit until done.
    if result.is_err() {
        job.0.abort();
    }
    result
}

/// Run a filesystem mutation with exclusive access.
///
/// # Errors
/// Returns an error if another mutation is running or the blocking work fails.
pub async fn mutation<T: Send + 'static>(
    name: &'static str,
    work: impl FnOnce() -> anyhow::Result<T> + Send + 'static,
) -> anyhow::Result<T> {
    let permit = MUTATIONS
        .clone()
        .try_acquire_owned()
        .map_err(|_| anyhow::anyhow!("{name}: another filesystem change is still running"))?;
    blocking(name, IO_TIMEOUT, move || {
        let _permit = permit;
        work()
    })
    .await
}

/// Apply the configured drive mappings as an exclusive filesystem mutation.
///
/// # Errors
/// Returns an error if another mutation is running or a mapping cannot be applied.
pub async fn apply_drive_mappings(config: crate::config::Config) -> anyhow::Result<()> {
    mutation("Applying drive mappings", move || {
        config.apply_drive_mappings()
    })
    .await
}

/// Spawn can touch slow executables/mounts. A child from a timed-out spawn is
/// killed on drop instead of being delivered late and left untracked.
///
/// # Errors
/// Returns an error if no worker is available, spawning fails, or the operation times out.
pub async fn spawn(mut command: tokio::process::Command) -> anyhow::Result<tokio::process::Child> {
    blocking("Starting process", IO_TIMEOUT, move || {
        Ok(command.kill_on_drop(true).spawn()?)
    })
    .await
}

/// The accepted game survives launcher shutdown; a late spawn is cleaned up.
///
/// # Errors
/// Returns an error if no worker is available, spawning fails, or the operation times out.
pub async fn spawn_game(
    mut command: tokio::process::Command,
) -> anyhow::Result<tokio::process::Child> {
    struct PendingChild(Option<tokio::process::Child>);
    impl Drop for PendingChild {
        fn drop(&mut self) {
            if let Some(child) = &mut self.0 {
                let _ = child.start_kill();
            }
        }
    }
    let mut pending = blocking("Starting game", IO_TIMEOUT, move || {
        Ok(PendingChild(Some(command.kill_on_drop(false).spawn()?)))
    })
    .await?;
    pending
        .0
        .take()
        .ok_or_else(|| anyhow::anyhow!("spawned game process was unavailable"))
}

/// Run a command and require it to finish successfully before its deadline.
///
/// # Errors
/// Returns an error if spawning or waiting fails, the deadline expires, or the
/// command exits unsuccessfully.
pub async fn command_status(
    command: tokio::process::Command,
    name: &'static str,
    duration: Duration,
) -> anyhow::Result<()> {
    let mut child = spawn(command).await?;
    let status = deadline(name, duration, async { Ok(child.wait().await?) }).await?;
    if !status.success() {
        anyhow::bail!("{name} exited with {status}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn executor_shutdown_does_not_wait_for_blocked_jobs() {
        let (started, ready) = std::sync::mpsc::channel();
        let (release, wait) = std::sync::mpsc::channel();
        let (closed, done) = std::sync::mpsc::channel();
        let thread = std::thread::spawn(move || {
            let executor = <Executor as iced::Executor>::new().unwrap();
            executor.0.as_ref().unwrap().spawn_blocking(move || {
                started.send(()).unwrap();
                let _ = wait.recv_timeout(Duration::from_secs(5));
            });
            ready.recv_timeout(Duration::from_secs(2)).unwrap();
            drop(executor);
            closed.send(()).unwrap();
        });
        let result = done.recv_timeout(Duration::from_secs(2));
        let _ = release.send(());
        thread.join().unwrap();
        assert!(
            result.is_ok(),
            "executor waited for a blocking job at shutdown"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn blocking_work_does_not_prevent_async_deadlines() {
        let (release, wait) = std::sync::mpsc::channel();
        let result = blocking("slow probe", Duration::from_millis(50), move || {
            let _ = wait.recv_timeout(Duration::from_secs(2));
            Ok(())
        })
        .await;
        assert!(result.unwrap_err().to_string().contains("timed out"));
        assert!(WORKERS.available_permits() < 4);
        let _ = release.send(());
    }

    #[tokio::test]
    async fn worker_panics_are_reported() {
        let result = blocking::<()>("probe", IO_TIMEOUT, || panic!("failed probe")).await;
        assert!(result.unwrap_err().to_string().contains("worker failed"));
    }

    #[tokio::test]
    async fn cancelled_mutation_retains_exclusion_until_worker_finishes() {
        let (started, ready) = tokio::sync::oneshot::channel();
        let (release, wait) = std::sync::mpsc::channel();
        let job = tokio::spawn(mutation("slow save", move || {
            let _ = started.send(());
            let _ = wait.recv_timeout(Duration::from_secs(2));
            Ok(())
        }));
        ready.await.unwrap();
        job.abort();
        let _ = job.await;
        let error = mutation("another save", || Ok(())).await.unwrap_err();
        assert!(error.to_string().contains("still running"));
        let _ = release.send(());
    }

    #[tokio::test]
    async fn helper_failure_and_timeout_are_errors() {
        let mut fail = tokio::process::Command::new("sh");
        fail.args(["-c", "exit 7"]);
        let error = command_status(fail, "failing helper", IO_TIMEOUT)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("exit status: 7"));
        let mut slow = tokio::process::Command::new("sleep");
        slow.arg("10");
        let error = command_status(slow, "slow helper", Duration::from_millis(20))
            .await
            .unwrap_err();
        assert!(error.to_string().contains("timed out"));
    }
}
