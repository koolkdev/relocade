use futures_channel::oneshot;
use futures_util::{
    future::{join, join3},
    FutureExt,
};
use relocade_workers::{Pool, PoolError, PoolHandle, Worker};
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc, Condvar, Mutex,
};

#[derive(Default)]
struct Gate {
    open: Mutex<bool>,
    changed: Condvar,
}

impl Gate {
    fn wait(&self) {
        drop(
            self.changed
                .wait_while(self.open.lock().unwrap(), |open| !*open)
                .unwrap(),
        );
    }

    fn release(&self) {
        *self.open.lock().unwrap() = true;
        self.changed.notify_all();
    }
}

// Release blocked workers even when a coordinator assertion fails.
struct Release(Arc<Gate>);
impl Drop for Release {
    fn drop(&mut self) {
        self.0.release();
    }
}

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
    fn send_sync<T: Send + Sync>() {}
    send_sync::<PoolHandle>();
    let threads = vec![Worker::new(), Worker::new()];
    let mut supplied = Vec::new();
    for thread in &threads {
        supplied.push(
            thread
                .run(|| async { std::thread::current().id() })
                .await
                .unwrap(),
        );
    }
    let (pool, first) = Pool::new(threads).unwrap();
    let second = first.clone();
    let third = first.clone();
    let gate = Arc::new(Gate::default());
    let activity = Arc::new(Activity::default());
    let make_job = || {
        let (started, ready) = oneshot::channel();
        let gate = gate.clone();
        let activity = activity.clone();
        (
            move || {
                let _running = activity.enter();
                let id = std::thread::current().id();
                let _ = started.send(id);
                gate.wait();
                id
            },
            ready,
        )
    };
    let (a, a_ready) = make_job();
    let (b, b_ready) = make_job();
    let (c, mut c_ready) = make_job();
    let release = Release(gate.clone());
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
    let (pool, jobs) = Pool::new(vec![Worker::new()]).unwrap();
    let gate = Arc::new(Gate::default());
    let released = Arc::new(AtomicBool::new(false));
    let drained = Arc::new(AtomicBool::new(false));
    let release = Release(gate.clone());
    let captured = Released(released.clone());
    let worker_drained = drained.clone();
    let (started, ready) = oneshot::channel();
    let client = async move {
        let mut abandoned = Box::pin(jobs.execute(move || {
            let _captured = captured;
            let _ = started.send(());
            gate.wait();
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
    let (pool, jobs) = Pool::new(vec![Worker::new()]).unwrap();
    let released = Arc::new(AtomicBool::new(false));
    let captured = Released(released.clone());
    let mut queued = Box::pin(jobs.execute(move || drop(captured)));
    assert!(queued.as_mut().now_or_never().is_none());
    drop(pool);
    assert!(matches!(queued.await, Err(PoolError::Closed)));
    assert!(released.load(Ordering::SeqCst));
    assert!(matches!(jobs.execute(|| 42).await, Err(PoolError::Closed)));
}

#[cfg(feature = "compiler")]
async fn concurrent_compilations() {
    use relocade_compiler::Program;
    fn program(value: u32) -> Program {
        let mut program = Program::new();
        let helper = program.function::<fn() -> u32>(move |body, ()| body.return_(value));
        for name in ["first", "second"] {
            let caller = program.function::<fn() -> u32>(move |mut body, ()| {
                let value = helper.call(&mut body, ())?;
                body.return_(value.add(1))
            });
            program.export(name, caller).unwrap();
        }
        program
    }
    let expected = [
        program(7).compile().unwrap(),
        program(41).compile().unwrap(),
    ];
    let (pool, local) = Pool::new(vec![Worker::new(), Worker::new()]).unwrap();
    let remote = local.clone();
    // The driver stays on this thread; a separate coordinator submits through
    // the same Send handle, including when browser Worker handles are local.
    let coordinator = Worker::new();
    let remote = coordinator.run(move || async move { program(41).compile_with(&remote).await });
    let local = async move { program(7).compile_with(&local).await };
    let ((), (local, remote)) = join(pool.run(), join(local, remote)).await;
    assert_eq!(local.unwrap(), expected[0]);
    assert_eq!(remote.unwrap().unwrap(), expected[1]);
}

#[cfg(feature = "compiler")]
async fn concurrent_ir_construction() {
    use relocade_compiler::{Program, I32};
    let threads = vec![Worker::new(), Worker::new()];
    let mut supplied = Vec::new();
    for thread in &threads {
        supplied.push(
            thread
                .run(|| async { std::thread::current().id() })
                .await
                .unwrap(),
        );
    }
    let (pool, executor) = Pool::new(threads).unwrap();
    let gate = Arc::new(Gate::default());
    let release = Release(gate.clone());
    let mut program = Program::new();
    let mut started = Vec::new();
    for name in ["first", "second"] {
        let (sender, receiver) = oneshot::channel();
        started.push(receiver);
        let gate = gate.clone();
        let function = program.function::<fn(u32) -> u32>(move |mut body, (input,)| {
            let local = std::rc::Rc::new(input.add(1));
            let _ = sender.send(std::thread::current().id());
            gate.wait();
            let result = body.block::<I32>(|block, label| block.branch(&label, local.as_ref()))?;
            body.return_(result)
        });
        program.export(name, function).unwrap();
    }
    let control = async move {
        let ids = futures_util::future::try_join_all(started).await.unwrap();
        assert_ne!(ids[0], ids[1], "IR bodies overlap on distinct workers");
        assert!(ids.iter().all(|id| supplied.contains(id)));
        drop(release);
    };
    let compilation = async move { program.compile_with(&executor).await.unwrap() };
    let ((), (bytes, ())) = join(pool.run(), join(compilation, control)).await;
    assert!(!bytes.is_empty());
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

#[cfg(all(feature = "compiler", not(target_arch = "wasm32")))]
#[test]
fn multiple_compilations_share_a_pool_across_threads() {
    pollster::block_on(concurrent_compilations());
}

#[cfg(all(feature = "compiler", target_arch = "wasm32"))]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn multiple_compilations_share_a_pool_across_threads() {
    concurrent_compilations().await;
}

#[cfg(all(feature = "compiler", not(target_arch = "wasm32")))]
#[test]
fn ir_bodies_build_concurrently_on_supplied_workers() {
    pollster::block_on(concurrent_ir_construction());
}

#[cfg(all(feature = "compiler", target_arch = "wasm32"))]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn ir_bodies_build_concurrently_on_supplied_workers() {
    concurrent_ir_construction().await;
}
