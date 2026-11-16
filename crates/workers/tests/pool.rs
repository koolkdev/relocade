use futures_channel::oneshot;
use futures_util::{
    future::{join, join3},
    FutureExt,
};
use relocade_workers::{JobExecutor, Pool, PoolError, Thread};
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc,
};

#[path = "support/gate.rs"]
mod gate;
use gate::blocking_gate;

#[derive(Default)]
struct Activity {
    running: AtomicUsize,
    peak: AtomicUsize,
}

impl Activity {
    fn enter(self: &Arc<Self>) -> Running {
        let running = self.running.fetch_add(1, Ordering::SeqCst) + 1;
        self.peak.fetch_max(running, Ordering::SeqCst);
        Running(self.clone())
    }
}

struct Running(Arc<Activity>);
impl Drop for Running {
    fn drop(&mut self) {
        self.0.running.fetch_sub(1, Ordering::SeqCst);
    }
}

async fn supplied_workers() {
    #[cfg(target_arch = "wasm32")]
    assert!(relocade_workers::available_parallelism().is_ok());
    fn send_sync<T: Send + Sync>() {}
    send_sync::<JobExecutor>();
    let threads = vec![Thread::new(), Thread::new()];
    let mut supplied = Vec::new();
    for thread in &threads {
        supplied.push(
            thread
                .run(|| async {
                    #[cfg(target_arch = "wasm32")]
                    assert!(relocade_workers::available_parallelism().is_ok());
                    std::thread::current().id()
                })
                .await
                .unwrap(),
        );
    }
    let (pool, first) = Pool::new(threads).unwrap();
    let second = first.clone();
    let third = first.clone();
    let activity = Arc::new(Activity::default());
    let make_job = || {
        let (started, ready) = oneshot::channel();
        let (release, wait) = blocking_gate();
        let activity = activity.clone();
        (
            move || {
                let _running = activity.enter();
                let id = std::thread::current().id();
                let _ = started.send(id);
                wait();
                id
            },
            ready,
            release,
        )
    };
    let (a, a_ready, a_release) = make_job();
    let (b, b_ready, b_release) = make_job();
    let (c, mut c_ready, c_release) = make_job();
    let release = [a_release, b_release, c_release];
    let clients = async move {
        let control = async {
            let (a, b) = join(a_ready, b_ready).await;
            let (a, b) = (a.unwrap(), b.unwrap());
            assert_ne!(
                a, b,
                "two jobs must run concurrently on distinct supplied threads"
            );
            assert!(supplied.contains(&a) && supplied.contains(&b));
            assert_eq!(activity.running.load(Ordering::SeqCst), 2);
            assert_eq!(c_ready.try_recv().unwrap(), None);
            drop(release);
        };
        let ((a, b, c), ()) = join(
            join3(first.execute(a), second.execute(b), third.execute(c)),
            control,
        )
        .await;
        for id in [a.unwrap(), b.unwrap(), c.unwrap()] {
            assert!(supplied.contains(&id));
        }
        assert_eq!(activity.peak.load(Ordering::SeqCst), 2);
        assert_eq!(activity.running.load(Ordering::SeqCst), 0);
    };
    join(pool.run(), clients).await;
}

struct Released(Arc<AtomicBool>);
impl Drop for Released {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

async fn cancelled_waiters() {
    let (pool, jobs) = Pool::new(vec![Thread::new()]).unwrap();
    let (release, wait) = blocking_gate();
    let released = Arc::new(AtomicBool::new(false));
    let drained = Arc::new(AtomicBool::new(false));
    let captured = Released(released.clone());
    let worker_drained = drained.clone();
    let (started, ready) = oneshot::channel();
    let client = async move {
        let mut abandoned = Box::pin(jobs.execute(move || {
            let _captured = captured;
            let _ = started.send(());
            wait();
        }));
        assert!(abandoned.as_mut().now_or_never().is_none());
        ready.await.unwrap();
        drop(abandoned);
        assert!(!released.load(Ordering::SeqCst));
        let mut queued = Box::pin(jobs.execute(move || {
            assert!(released.load(Ordering::SeqCst));
            worker_drained.store(true, Ordering::SeqCst);
        }));
        assert!(queued.as_mut().now_or_never().is_none());
        drop(queued);
        drop(jobs);
        drop(release);
    };
    join(pool.run(), client).await;
    assert!(
        drained.load(Ordering::SeqCst),
        "shutdown drains accepted jobs without waiters"
    );
}

async fn closed_pool() {
    assert!(matches!(Pool::new(Vec::new()), Err(PoolError::NoThreads)));
    let (pool, jobs) = Pool::new(vec![Thread::new()]).unwrap();
    let released = Arc::new(AtomicBool::new(false));
    let captured = Released(released.clone());
    let mut queued = Box::pin(jobs.execute(move || drop(captured)));
    assert!(queued.as_mut().now_or_never().is_none());
    drop(pool);
    assert!(matches!(queued.await, Err(PoolError::Closed)));
    assert!(released.load(Ordering::SeqCst));
    assert!(matches!(jobs.execute(|| 42).await, Err(PoolError::Closed)));
}

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn supplied_threads_share_one_concurrency_limit() {
    pollster::block_on(supplied_workers());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn supplied_threads_share_one_concurrency_limit() {
    supplied_workers().await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn dropped_waiters_keep_their_slot_and_shutdown_drains() {
    pollster::block_on(cancelled_waiters());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn dropped_waiters_keep_their_slot_and_shutdown_drains() {
    cancelled_waiters().await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn empty_and_closed_pools_report_errors() {
    pollster::block_on(closed_pool());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn empty_and_closed_pools_report_errors() {
    closed_pool().await;
}
