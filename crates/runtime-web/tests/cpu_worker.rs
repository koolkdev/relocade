use futures_channel::oneshot;
use relocade_machine::{MachineConfig, ReadMemory};
use relocade_runtime::{Hook, Runtime, RuntimeState};
use relocade_x86::{
    Compiler, CpuState, ExecutionExit, ExecutionProfile, PhysicalMapping, PhysicalMemoryMap,
    Segments,
};
use std::{
    future::Future,
    ops::ControlFlow,
    pin::pin,
    task::{Context, Poll, Waker},
};

#[cfg(not(target_arch = "wasm32"))]
use relocade_runtime_wasmtime::WasmtimeBackend as Backend;
#[cfg(target_arch = "wasm32")]
use relocade_runtime_web::WebBackend as Backend;

// The boxed callback is deliberately non-Send: host state is created and used
// on the CPU worker. Only input bytes and shared memory cross threads.
struct State {
    input: Option<oneshot::Receiver<u8>>,
    waiting: Box<dyn Fn()>,
}

async fn runtime(
    input: oneshot::Receiver<u8>,
    waiting: impl Fn() + 'static,
) -> Runtime<Backend<RuntimeState<State>>> {
    let state = RuntimeState::new(State {
        input: Some(input),
        waiting: Box::new(waiting),
    });
    #[cfg(target_arch = "wasm32")]
    let backend = Backend::with_state(state);
    #[cfg(not(target_arch = "wasm32"))]
    let backend = Backend::with_state(state).unwrap();
    let config = MachineConfig::new(
        Compiler::new(ExecutionProfile::Real16).target(),
        CpuState {
            eip: 0x100,
            segments: Segments::real_mode(),
            ..CpuState::default()
        },
        0x10000,
        PhysicalMemoryMap::new([(0..=0xffff, PhysicalMapping::Ram { backing_offset: 0 })]).unwrap(),
    )
    .unwrap();
    let mut runtime = Runtime::new(backend, config).await.unwrap();
    runtime
        .machine()
        .memory()
        .write(
            0x100,
            &[
                0xc6, 0x06, 0, 4, 1,    // MOV byte [0400],1: visible through shared memory.
                0x40, // INC AX
                0xeb, 0xf8, // JMP 0100
            ],
        )
        .unwrap();
    runtime
        .install_hooks([Hook::<State>::new(0x200, |call| {
            let input = call.state_mut().input.take().unwrap();
            call.defer(async move |call| {
                (call.state().waiting)();
                let byte = input.await.unwrap();
                call.cpu().set_ax(u16::from(byte));
                ControlFlow::Break(ExecutionExit::HostReturn)
            })
        })])
        .await
        .unwrap();
    // Scheduling belongs to the worker loop. A ready guest run does not add a
    // scheduler wait, even when invoked through the deferred-service API.
    assert!(matches!(
        pin!(runtime.run_async(256))
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop())),
        Poll::Ready(Ok(ExecutionExit::BudgetExhausted))
    ));
    assert_eq!(runtime.shared_memory().read_u8(0x400).unwrap(), 1);
    runtime
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn cpu_thread_owns_runtime_and_awaits_input_from_another_thread() {
    use std::{sync::mpsc, time::Duration};
    let (input, receiver) = oneshot::channel();
    let (ready, memory) = mpsc::channel();
    let (read, commands) = mpsc::channel();
    let (waiting, reading) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        pollster::block_on(async move {
            let mut runtime = runtime(receiver, move || waiting.send(()).unwrap()).await;
            ready.send(runtime.shared_memory()).unwrap();
            loop {
                if commands.try_recv().is_ok() {
                    runtime.machine().cpu().set_eip(0x200);
                }
                match runtime.run_async(256).await.unwrap() {
                    ExecutionExit::BudgetExhausted => {}
                    ExecutionExit::HostReturn => return runtime.machine().cpu().snapshot(),
                    exit => panic!("unexpected guest exit: {exit:?}"),
                }
            }
        })
    });
    let memory = memory.recv_timeout(Duration::from_secs(30)).unwrap();
    assert_eq!(memory.read_u8(0x400).unwrap(), 1);
    read.send(()).unwrap();
    reading.recv_timeout(Duration::from_secs(10)).unwrap();
    input.send(42).unwrap();
    let cpu = worker.join().unwrap();
    assert_eq!(cpu.registers.eax, 42);
    assert!(cpu.instruction_count >= 256);
}

#[cfg(target_arch = "wasm32")]
mod browser {
    use super::*;
    use std::cell::{Cell, RefCell};
    use wasm_bindgen::prelude::*;

    wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

    thread_local! {
        static READ: Cell<bool> = const { Cell::new(false) };
        static INPUT: RefCell<Option<oneshot::Sender<u8>>> = const { RefCell::new(None) };
    }

    #[wasm_bindgen(module = "/tests/cpu_worker.js")]
    extern "C" {
        fn started(memory: &js_sys::WebAssembly::Memory);
        fn waiting();
        #[wasm_bindgen(catch)]
        async fn run_in_worker() -> Result<JsValue, JsValue>;
    }

    #[wasm_bindgen]
    pub fn request_read() {
        READ.set(true);
    }

    #[wasm_bindgen]
    pub fn provide_input(byte: u8) {
        INPUT.with_borrow_mut(|input| input.take().unwrap().send(byte).unwrap());
    }

    #[wasm_bindgen]
    pub async fn run_cpu() -> Vec<u32> {
        let (input, receiver) = oneshot::channel();
        INPUT.with_borrow_mut(|slot| *slot = Some(input));
        let mut runtime = runtime(receiver, waiting).await;
        let memory = runtime.shared_memory();
        started(memory.backing().memory());
        loop {
            if READ.get() {
                runtime.machine().cpu().set_eip(0x200);
            }
            match runtime.run_async(256).await.unwrap() {
                ExecutionExit::BudgetExhausted => {}
                ExecutionExit::HostReturn => {
                    let cpu = runtime.machine().cpu().snapshot();
                    return vec![cpu.registers.eax, (cpu.instruction_count >= 256).into()];
                }
                exit => panic!("unexpected guest exit: {exit:?}"),
            }
            relocade_runtime_web::yield_to_event_loop().await.unwrap();
        }
    }

    #[wasm_bindgen_test::wasm_bindgen_test]
    async fn dedicated_cpu_worker_receives_commands_and_resumes_deferred_input() {
        let result = js_sys::Uint32Array::new(&run_in_worker().await.unwrap()).to_vec();
        assert_eq!(result, [42, 1]);
    }
}
