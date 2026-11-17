use futures_lite::future::poll_once;
use relocade_clock_test_support::ManualClock;
use relocade_frontend::{
    input::InputEvent,
    keyboard::{ElementState, Key, KeyCode, KeyLocation, KeyboardEvent, NativeKey},
};
use relocade_machine::{
    worker::{wake_connection, WakeHost},
    ExecutionInterrupt, GuestTimeConfig, Hook, Machine, Runner,
};
use relocade_pc::{configuration, Pc};
use relocade_x86::{ExecutionExit, HostExit};
use std::{
    future::{pending, poll_fn, Future},
    num::NonZeroU64,
    pin::{pin, Pin},
    task::Poll,
    time::Duration,
};

async fn paced_runner(host: &ManualClock) -> (Machine, WakeHost<ExecutionInterrupt>, Runner) {
    let mut machine = Machine::new(configuration::memory()).unwrap();
    let cpu = machine.create_cpu(configuration::cpu(), None).unwrap();
    let (wake, endpoint) = wake_connection(cpu.interrupt_handle());
    let config =
        GuestTimeConfig::new(NonZeroU64::new(1_000_000).unwrap()).with_host_clock(host.clock());
    let runner = Runner::with_guest_time(config, cpu, endpoint)
        .await
        .unwrap();
    (machine, wake, runner)
}

async fn wait_for_pacing(mut session: Pin<&mut impl Future>, host: &ManualClock) {
    poll_fn(|cx| {
        assert!(session.as_mut().poll(cx).is_pending());
        if host.next_deadline().is_some() {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    })
    .await;
}

async fn keyboard_at_guest_receipt_time() {
    let host = ManualClock::new();
    let (machine, _wake, runner) = paced_runner(&host).await;
    let clock = runner.clock().clone();
    let (ready, input) = async_channel::bounded(1);
    let mut session = pin!(runner.run(pending::<()>(), async move |runner| {
        let mut pc = Pc::new(runner).await.unwrap();
        ready.try_send(pc.keyboard_input()).unwrap();
        let cpu = pc.cpu();
        // Bumpy's PIT divisor, IRQ0/IRQ1 enabled, then a blocking BIOS key read.
        cpu.memory()
            .view()
            .write(
                0x1000,
                &[
                    0xb0, 0x36, 0xe6, 0x43, 0xb0, 0x51, 0xe6, 0x40, 0xb0, 0x09, 0xe6, 0x40, 0xb0,
                    0xfc, 0xe6, 0x21, 0xfb, 0x90, 0x31, 0xc0, 0xcd, 0x16,
                ],
            )
            .unwrap();
        let state = cpu.cpu_state();
        state.set_eip(0x1000);
        state.set_sp(0x8000);
        let exit = cpu.exit_handle();
        cpu.install([Hook::host(0x1016, async move || exit.request())])
            .await
            .unwrap();
        assert_eq!(pc.run().await, ExecutionExit::Host(HostExit::Return));
        (state.ax(), pc.clock().now())
    }));
    wait_for_pacing(session.as_mut(), &host).await;
    host.advance_by(Duration::from_millis(7));
    let keyboard = input.try_recv().unwrap();
    let boundary = clock.now();
    keyboard
        .unbounded_send(InputEvent {
            host_time: host.host_now(),
            event: KeyboardEvent::Key {
                physical_key: KeyCode::KeyA.into(),
                logical_key: Key::Character("a".into()),
                location: KeyLocation::Standard,
                text: Some("a".into()),
                state: ElementState::Pressed,
                repeat: false,
            },
        })
        .unwrap();
    host.advance_by(Duration::from_millis(2));
    assert_eq!(clock.now(), boundary);
    let (key, completed) = session.await.unwrap();
    assert_eq!(key, 0x1e61);
    assert!(
        completed >= boundary
            && completed < clock.deadline_after(boundary, Duration::from_millis(1))
    );
    assert_eq!(host.next_deadline(), None);
    machine.shutdown().await.unwrap();
}

async fn queued_arrow_taps() {
    let host = ManualClock::new();
    let (machine, _wake, mut runner) = paced_runner(&host).await;
    let memory = runner.memory();
    let state = runner.cpu().cpu_state();
    let (ready, input) = async_channel::bounded(1);
    let (stop, stopped) = async_channel::bounded::<()>(1);
    let mut session = pin!(runner.run(stopped.recv(), async |runner| {
        let mut pc = Pc::new(runner).await.unwrap();
        // Enable only IRQ1, then wait in HLT between interrupts.
        memory
            .view()
            .write(
                0x1000,
                &[0xb0, 0xfd, 0xe6, 0x21, 0xfb, 0x90, 0xf4, 0xeb, 0xfd],
            )
            .unwrap();
        memory.view().write(9 * 4, &[0, 0x20, 0, 0]).unwrap();
        // IN AL,60h; STOSB; MOV AL,20h; OUT 20h,AL; IRET.
        memory
            .view()
            .write(0x2000, &[0xe4, 0x60, 0xaa, 0xb0, 0x20, 0xe6, 0x20, 0xcf])
            .unwrap();
        state.set_eip(0x1000);
        state.set_sp(0x8000);
        state.set_di(0x800);
        ready.try_send(pc.keyboard_input()).unwrap();
        pc.run().await
    }));
    wait_for_pacing(session.as_mut(), &host).await;
    let keyboard = input.try_recv().unwrap();
    // The execution worker is stopped while eight transitions arrive over 14 ms.
    for index in 0..8 {
        if index != 0 {
            host.advance_by(Duration::from_millis(2));
        }
        let state = if index % 2 == 0 {
            ElementState::Pressed
        } else {
            ElementState::Released
        };
        keyboard
            .unbounded_send(InputEvent {
                host_time: host.host_now(),
                event: KeyboardEvent::Key {
                    physical_key: KeyCode::ArrowUp.into(),
                    logical_key: Key::Unidentified(NativeKey::Unidentified),
                    location: KeyLocation::Standard,
                    text: None,
                    state,
                    repeat: false,
                },
            })
            .unwrap();
    }
    // Resume at the real pacing boundaries. Only the guest ISR reads port 60h.
    // Bound the wait so a missing release produces a scan-log assertion.
    for _ in 0..12 {
        assert!(poll_once(session.as_mut()).await.is_none());
        host.advance_by(Duration::from_millis(4));
    }
    stop.try_send(()).unwrap();
    assert_eq!(session.await, None);
    let mut scans = vec![0; usize::from(state.di() - 0x800)];
    memory.view().read(0x800, &mut scans).unwrap();
    assert_eq!(scans, [0xe0, 0x48, 0xe0, 0xc8].repeat(4));
    machine.shutdown().await.unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn queued_keyboard_input_uses_guest_receipt_time_during_pacing() {
    pollster::block_on(keyboard_at_guest_receipt_time());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn queued_keyboard_input_uses_guest_receipt_time_during_pacing() {
    keyboard_at_guest_receipt_time().await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn guest_irq1_reads_every_release_after_a_keyboard_backlog() {
    pollster::block_on(queued_arrow_taps());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn guest_irq1_reads_every_release_after_a_keyboard_backlog() {
    queued_arrow_taps().await;
}
