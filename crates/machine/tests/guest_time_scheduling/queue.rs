//! Ordering between a machine wake inside guest I/O and a due device timer.

use super::*;
use relocade_machine::{GuestTimeConfig, PortRead, PortWrite};
use std::task::{Context, Waker};

async fn due_device_before_next_block() {
    let setup = configuration();
    let mut machine = relocade_machine::Machine::new(setup.machine).unwrap();
    let cpu = machine.create_cpu(setup.cpu, None).unwrap();
    let worker = cpu.spawn_with_guest_time(
        || GuestTimeConfig::new(NonZeroU64::new(1000).unwrap()),
        async |mut runner| {
            // OUT is one block. The subsequent JMP supplies a second, observable turn.
            let code = [0xe6, 0x80, 0xeb, 0xfe];
            prepare_cpu(&mut runner, &code).await;
            let block = Compiler::new(ExecutionProfile::Real16)
                .compile_block(0x100, &code, 1)
                .unwrap();
            runner
                .cpu()
                .install_module(
                    &block.bytes,
                    &[DispatchEntry {
                        key: 0x100,
                        export: &block.entry,
                    }],
                )
                .await
                .unwrap();
            let clock = runner.clock().clone();
            let due = clock.deadline_after(clock.now(), Duration::from_millis(1));
            let timer = Rc::new(RefCell::new(Deadlines {
                clock: clock.clone(),
                deadlines: vec![due],
                observed: Vec::new(),
                exit: Some(runner.cpu().exit_handle()),
                exit_after_waits: true,
            }));
            runner
                .connect(timer.clone(), |io| {
                    io.bind(
                        0x80,
                        &PortRead::new(|_, _, _| 0),
                        &PortWrite::new(move |_, _, _, _| {
                            // Changing a guest wait wakes the machine before OUT
                            // publishes retirement and wakes the one-tick timer.
                            let mut wait = pin!(clock.sleep(Duration::from_secs(1)));
                            assert!(wait
                                .as_mut()
                                .poll(&mut Context::from_waker(Waker::noop()))
                                .is_pending());
                        }),
                    );
                })
                .await;
            assert_eq!(
                runner.run_cpu().await,
                ExecutionExit::Host(HostExit::Return)
            );
            // The activity requests exit as soon as it observes the deadline.
            assert_eq!(timer.borrow().observed, [due]);
            assert_eq!(runner.cpu().cpu_state().instruction_count(), 1);
        },
    );
    assert_eq!(worker.join().await.unwrap(), Some(()));
    machine.shutdown().await.unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_device_due_during_io_runs_before_the_next_guest_block() {
    pollster::block_on(due_device_before_next_block());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn a_device_due_during_io_runs_before_the_next_guest_block() {
    due_device_before_next_block().await;
}
