use super::*;
use futures_channel::oneshot;
use futures_lite::future::poll_once;
use futures_util::future::{join, FutureExt};
use relocade_machine::{timer_service, worker::wait_for_host, ExecutionWorker, Hook};
use relocade_x86::HostExit;
use std::{
    cell::{Cell, RefCell},
    future::pending,
    pin::pin,
    rc::Rc,
};

async fn retained_call() {
    for wait_in_call in [false, true] {
        let (clock, _timers) = timer_service();
        let worker = ExecutionWorker::spawn(clock, async move |runner| {
            // Host entry at 0100; then INC AX; JMP 0105, the completion hook.
            let irq = Rc::new(RefCell::new(irq::Irq::default()));
            let io = IoBus::new([runner.connect(irq.clone(), [], []).await]);
            let mut cpu = cpu_with_runner(&[0xeb, 0, 0x40, 0xeb, 0], false, io, Some(runner)).await;
            let state = cpu.cpu_state();
            state.set_sp(0x8000);
            cpu.memory().view().write_u32(0x180, 0x300).unwrap();
            cpu.memory().view().write(0x300, &[0x43, 0xcf]).unwrap(); // INC BX; IRET.
            let call_state = state.clone();
            let exit = cpu.exit_handle();
            let (send, receive) = oneshot::channel::<()>();
            let receive = receive.shared();
            cpu.install([
                Hook::host(0x100, async move || {
                    if wait_in_call {
                        call_state.set_interrupts_enabled(true);
                        wait_for_host(receive.clone()).await.unwrap();
                    } else {
                        receive.clone().await.unwrap();
                    }
                    call_state.set_eip(0x102);
                }),
                Hook::host(0x105, async move || exit.request()),
            ])
            .await
            .unwrap();
            cpu.interrupt_handle().take();
            let mut run = Box::pin(cpu.run(u64::MAX));
            assert!(poll_once(run.as_mut()).await.is_none());
            // The gate must not prevent polling a retained service that itself
            // needs to release a host wait. Replace its enclosing run waiter.
            if wait_in_call {
                irq.borrow_mut().raise();
                assert!(poll_once(run.as_mut()).await.is_none());
                assert_eq!(
                    irq.borrow().acknowledged,
                    0,
                    "host work must finish before IRQ suspension"
                );
                drop(run);
                send.send(()).unwrap();
                assert_eq!(
                    runner.run_cpu(&mut cpu).await,
                    ExecutionExit::Host(HostExit::Return)
                );
            } else {
                // Register a sibling host wait while the existing CPU run is
                // suspended in an ordinary service, then make that service ready.
                let mut resource = Box::pin(wait_for_host(pending::<()>()));
                assert!(poll_once(resource.as_mut()).await.is_none());
                send.send(()).unwrap();
                assert!(poll_once(run.as_mut()).await.is_none());
                assert_eq!(state.eip(), 0x102, "the service completed");
                assert_eq!(state.instruction_count(), 0, "guest entry must still wait");
                drop(run);
                let mut replacement = pin!(runner.run_cpu(&mut cpu));
                assert!(poll_once(replacement.as_mut()).await.is_none());
                assert!(poll_once(replacement.as_mut()).await.is_none());
                assert_eq!(state.instruction_count(), 0);
                drop(resource);
                assert_eq!(replacement.await, ExecutionExit::Host(HostExit::Return));
            }
            assert_eq!(state.ax(), 1);
            assert_eq!(state.instruction_count(), if wait_in_call { 4 } else { 2 });
            assert_eq!(
                state.bx(),
                u16::from(wait_in_call),
                "the queued ISR must run after host readiness"
            );
            assert_eq!(state.sp(), 0x8000);
        });
        assert_eq!(worker.join().await.unwrap(), Some(()));
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn host_waits_gate_guest_reentry_without_parking_retained_services() {
    pollster::block_on(retained_call());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn host_waits_gate_guest_reentry_without_parking_retained_services() {
    retained_call().await;
}

async fn termination() {
    let (clock, _timers) = timer_service();
    let worker = ExecutionWorker::spawn(clock, async |runner| {
        let mut cpu = cpu_with_runner(&[0xeb, 0xfe], false, IoBus::default(), Some(runner)).await;
        let exit = cpu.exit_handle();
        let (send, receive) = oneshot::channel::<()>();
        let receive = receive.shared();
        cpu.install([Hook::host(0x100, async move || {
            receive.clone().await.unwrap();
            exit.request();
        })])
        .await
        .unwrap();
        cpu.interrupt_handle().take();
        let mut run = pin!(cpu.run(u64::MAX));
        assert!(poll_once(run.as_mut()).await.is_none());
        let mut resource = pin!(wait_for_host(pending::<()>()));
        assert!(poll_once(resource.as_mut()).await.is_none());
        send.send(()).unwrap();
        assert_eq!(
            poll_once(run.as_mut()).await,
            Some(ExecutionExit::Host(HostExit::Return)),
            "termination must not wait for an unrelated host resource",
        );
    });
    assert_eq!(worker.join().await.unwrap(), Some(()));
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn host_resources_do_not_delay_guest_termination() {
    pollster::block_on(termination());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn host_resources_do_not_delay_guest_termination() {
    termination().await;
}

async fn compilation() {
    use relocade_wasm_runtime::{Backend, WasmBackend, WasmCompiler};

    let (clock, _timers) = timer_service();
    let worker = ExecutionWorker::spawn(clock, async |runner| {
        let mut cpu = cpu_with_runner(&[0xeb, 0xfe], true, IoBus::default(), Some(runner)).await;
        let finished = Rc::new(Cell::new(false));
        let hook_finished = finished.clone();
        let state = cpu.cpu_state();
        let exit = cpu.exit_handle();
        cpu.install([Hook::host(0x100, async move || {
            assert!(
                hook_finished.get(),
                "compilation must finish before guest reentry"
            );
            exit.request();
        })])
        .await
        .unwrap();
        let backend = Backend::new().unwrap();
        let compile = async {
            let compiler = backend.compiler();
            let mut compiled = pin!(compiler.compile(b"\0asm\x01\0\0\0"));
            let first = poll_once(compiled.as_mut()).await;
            #[cfg(target_arch = "wasm32")]
            assert!(
                first.is_none(),
                "browser compilation needs its Promise callback"
            );
            match first {
                Some(result) => result.unwrap(),
                None => compiled.await.unwrap(),
            };
            finished.set(true);
        };
        let ((), exit) = join(compile, runner.run_cpu(&mut cpu)).await;
        assert_eq!(exit, ExecutionExit::Host(HostExit::Return));
        assert_eq!(state.instruction_count(), 0);
    });
    assert_eq!(worker.join().await.unwrap(), Some(()));
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn compilation_can_finish_alongside_a_ready_cpu() {
    pollster::block_on(compilation());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn compilation_can_finish_alongside_a_ready_cpu() {
    compilation().await;
}
