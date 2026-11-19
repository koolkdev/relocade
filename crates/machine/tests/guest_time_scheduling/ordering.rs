//! Ordering of device wakes, guest execution and idle deadline advancement.

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

async fn retained_hook_wakes_timer_owner() {
    use futures_channel::oneshot;
    use futures_util::future::{select, Either};

    let setup = configuration();
    let mut machine = relocade_machine::Machine::new(setup.machine).unwrap();
    let cpu = machine.create_cpu(setup.cpu, None).unwrap();
    let worker = cpu.spawn_with_guest_time(
        || GuestTimeConfig::new(NonZeroU64::new(1000).unwrap()),
        async |mut runner| {
            let (send, signal) = oneshot::channel();
            let send = RefCell::new(Some(send));
            runner
                .cpu()
                .install([Hook::host(0x100, async move || {
                    relocade_machine::yield_now().await;
                    send.take().unwrap().send(()).unwrap();
                    std::future::pending::<()>().await;
                })])
                .await
                .unwrap();
            let exit = runner.cpu().exit_handle();
            let clock = runner.clock().clone();
            let due = clock.deadline_after(clock.now(), Duration::from_secs(20));
            runner
                .spawn_activity(async move {
                    let timer = pin!(clock.sleep_until(due));
                    assert!(matches!(select(signal, timer).await, Either::Left(_)));
                    assert!(
                        clock.now() < due,
                        "a woken device may cancel its timer before idle time advances"
                    );
                    exit.request();
                })
                .await;
            assert_eq!(
                runner.run_cpu().await,
                ExecutionExit::Host(HostExit::Return)
            );
        },
    );
    assert_eq!(worker.join().await.unwrap(), Some(()));
    machine.shutdown().await.unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_device_woken_by_a_retained_hook_runs_before_idle_time_advances() {
    pollster::block_on(retained_hook_wakes_timer_owner());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn a_device_woken_by_a_retained_hook_runs_before_idle_time_advances() {
    retained_hook_wakes_timer_owner().await;
}

async fn new_activity_registers_earlier_deadline() {
    let setup = configuration();
    let mut machine = relocade_machine::Machine::new(setup.machine).unwrap();
    let cpu = machine.create_cpu(setup.cpu, None).unwrap();
    let worker = cpu.spawn_with_guest_time(
        || GuestTimeConfig::new(NonZeroU64::new(1000).unwrap()),
        async |runner| {
            let clock = runner.clock();
            let start = clock.now();
            let later = clock.deadline_after(start, Duration::from_secs(20));
            runner.spawn_activity(clock.sleep_until(later)).await;
            let earlier = clock.deadline_after(start, Duration::from_secs(1));
            let (send, observed) = futures_channel::oneshot::channel();
            let clock = clock.clone();
            runner
                .spawn_activity(async move {
                    clock.sleep_until(earlier).await;
                    send.send(clock.now()).unwrap();
                })
                .await;
            assert_eq!(observed.await.unwrap(), earlier);
        },
    );
    assert_eq!(worker.join().await.unwrap(), Some(()));
    machine.shutdown().await.unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_new_activity_can_register_an_earlier_idle_deadline() {
    pollster::block_on(new_activity_registers_earlier_deadline());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn a_new_activity_can_register_an_earlier_idle_deadline() {
    new_activity_registers_earlier_deadline().await;
}
