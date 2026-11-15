//! Compiler integration with shared worker pools on native and browser targets.

use futures_channel::oneshot;
use futures_util::future::join;
use relocade_workers::{Pool, Thread};
use std::sync::{Arc, Condvar, Mutex};

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
    let (pool, local) = Pool::new(vec![Thread::new(), Thread::new()]).unwrap();
    let remote = local.clone();
    // The driver stays on this thread; a separate coordinator submits through
    // the same Send handle, including when browser Thread handles are local.
    let coordinator = Thread::new();
    let remote = coordinator.run(move || async move { program(41).compile_with(&remote).await });
    let local = async move { program(7).compile_with(&local).await };
    let ((), (local, remote)) = join(pool.run(), join(local, remote)).await;
    assert_eq!(local.unwrap(), expected[0]);
    assert_eq!(remote.unwrap().unwrap(), expected[1]);
}

async fn concurrent_ir_construction() {
    use relocade_compiler::{Program, I32};
    let threads = vec![Thread::new(), Thread::new()];
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
fn multiple_compilations_share_a_pool_across_threads() {
    pollster::block_on(concurrent_compilations());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn multiple_compilations_share_a_pool_across_threads() {
    concurrent_compilations().await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn ir_bodies_build_concurrently_on_supplied_workers() {
    pollster::block_on(concurrent_ir_construction());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn ir_bodies_build_concurrently_on_supplied_workers() {
    concurrent_ir_construction().await;
}
