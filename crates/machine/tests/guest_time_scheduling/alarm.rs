//! Hardware alarm registration at I/O boundaries and deferred device callbacks.

use super::*;
use futures_channel::oneshot;
use relocade_machine::{
    worker::wake_connection, Alarm, DeviceCallbacks, GuestTimeDriver, PortRead, PortWrite,
};
use std::task::Waker;

#[derive(Default)]
struct Timer {
    alarm: Option<Alarm>,
    fired: usize,
}

impl Device for Timer {}

async fn reprogram_and_dispatch() {
    let setup = configuration();
    let mut machine = relocade_machine::Machine::new(setup.machine).unwrap();
    let cpu = machine.create_cpu(setup.cpu, None).unwrap();
    let (_host, endpoint) = wake_connection(cpu.interrupt_handle());
    // Drive this guest clock explicitly so the test can cancel an expired alarm
    // between its wake and the next device turn.
    let driver = Rc::new(GuestTimeDriver::new(NonZeroU64::new(1000).unwrap()));
    let runner = Runner::new(driver.clock(), cpu, endpoint).await.unwrap();
    let result = runner
        .run(std::future::pending::<()>(), async |mut runner| {
            let _binding = driver.bind(runner.cpu(), Waker::noop());
            runner.cpu().instruction_target().set(Some(4));
            prepare_cpu(&mut runner, &[0xe6, 0x80, 0xeb, 0xfe]).await; // OUT; JMP short self
            let timer = Rc::new(RefCell::new(Timer::default()));
            let (done, twice) = oneshot::channel();
            let mut done = Some(done);
            timer.borrow_mut().alarm = Some(runner.alarm({
                let timer = Rc::downgrade(&timer);
                let clock = runner.clock().clone();
                move || {
                    let timer = timer.upgrade().unwrap();
                    let mut timer = timer.borrow_mut();
                    timer.fired += 1;
                    if timer.fired == 1 {
                        // An immediately due replacement runs on a later dispatch,
                        // after this mutable device borrow has been released.
                        timer.alarm.as_ref().unwrap().replace(Some(clock.now()));
                    } else {
                        done.take().unwrap().send(()).unwrap();
                    }
                }
            }));
            runner.io().connect(timer.clone(), |io| {
                let driver = driver.clone();
                let write = DeviceCallbacks::new(&timer).write(move |_, timer, _| {
                    for tick in [2, 9] {
                        let at = Timestamp::from_ticks(tick);
                        timer.alarm.as_ref().unwrap().replace(Some(at));
                        assert_eq!(driver.next_deadline(), Some(at));
                    }
                });
                io.bind(
                    0x80,
                    &PortRead::new(|_, _, _| 0),
                    &PortWrite::low_byte(write),
                );
            });
            assert_eq!(runner.run_cpu().await, ExecutionExit::BudgetExhausted);
            assert_eq!(runner.clock().now(), Timestamp::from_ticks(4));
            {
                let timer = timer.borrow_mut();
                assert!(driver.advance_to_next_deadline());
                assert_eq!(runner.clock().now(), Timestamp::from_ticks(9));
                timer.alarm.as_ref().unwrap().replace(None);
            }
            relocade_machine::yield_now().await;
            assert_eq!(
                timer.borrow().fired,
                0,
                "cancel suppressed the queued expiry"
            );
            timer
                .borrow()
                .alarm
                .as_ref()
                .unwrap()
                .replace(Some(runner.clock().now()));
            twice.await.unwrap();
            assert_eq!(timer.borrow().fired, 2);
            let alarm = timer.borrow_mut().alarm.take().unwrap();
            alarm.replace(Some(Timestamp::from_ticks(20)));
            drop(alarm);
            assert_eq!(driver.next_deadline(), None);
            let retained = runner.alarm(|| panic!("alarm fired after its session ended"));
            retained.replace(Some(Timestamp::from_ticks(30)));
            retained
        })
        .await;
    let retained = result.unwrap();
    assert_eq!(driver.next_deadline(), None);
    retained.replace(Some(Timestamp::from_ticks(40)));
    assert_eq!(driver.next_deadline(), None);
    machine.shutdown().await.unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn port_writes_replace_guest_alarms_and_callbacks_run_on_device_turns() {
    pollster::block_on(reprogram_and_dispatch());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn port_writes_replace_guest_alarms_and_callbacks_run_on_device_turns() {
    reprogram_and_dispatch().await;
}
