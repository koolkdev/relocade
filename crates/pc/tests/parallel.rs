use futures_io::AsyncRead;
use relocade_clock_test_support::ManualClock;
use relocade_machine::{worker::wake_connection, Hook, Runner};
use relocade_parallel::{
    centronics::{ControlEvent, Controls, Input, Status},
    Centronics,
};
use relocade_pc::{Configuration, ParallelPort, Pc};
use relocade_x86::{ExecutionExit, HostExit};
use std::{
    cell::RefCell,
    future::{poll_fn, Future},
    pin::{pin, Pin},
    rc::Rc,
    task::{Context, Poll, Waker},
    time::Duration,
};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

async fn read(input: &mut Input) -> u8 {
    let mut byte = [0];
    let count = poll_fn(|cx| Pin::new(&mut *input).poll_read(cx, &mut byte))
        .await
        .unwrap();
    assert_eq!(count, 1);
    byte[0]
}

fn pending(pc: &mut Pc) {
    // Reach the firmware wait without mistaking a runner scheduling yield for it.
    // Device activities still belong to the surrounding execution session.
    for _ in 0..16 {
        match pin!(pc.cpu().run()).poll(&mut Context::from_waker(Waker::noop())) {
            Poll::Pending => return,
            Poll::Ready(ExecutionExit::Interrupted) => {}
            result => panic!("expected a pending BIOS call, got {result:?}"),
        }
    }
    panic!("CPU did not reach its firmware wait");
}

fn request(pc: &mut Pc, ax: u16, index: u16) {
    let state = pc.cpu().cpu_state();
    state.set_eip(0x1000);
    state.set_sp(0x8000);
    state.set_eax(0xaabb_0000 | u32::from(ax));
    state.set_ebx(0x1234_5678);
    state.set_ecx(0x8765_4321);
    state.set_edx(u32::from(index));
    state.set_eflags(0x857); // Carry set, caller IF clear.
}

async fn complete(pc: &mut Pc, ax: u16, index: u16) {
    assert_eq!(pc.run().await, ExecutionExit::Host(HostExit::Return));
    let state = pc.cpu().cpu_state();
    assert_eq!(state.eax(), 0xaabb_0000 | u32::from(ax));
    assert_eq!(state.ebx(), 0x1234_5678);
    assert_eq!(state.ecx(), 0x8765_4321);
    assert_eq!(state.edx(), u32::from(index));
    assert_eq!(state.sp(), 0x8000);
    assert_eq!(state.eflags(), 0x857);
}

async fn bios_parallel_io() {
    let clock = ManualClock::default();
    let mut machine = relocade_machine::Machine::new(relocade_pc::configuration::memory()).unwrap();
    let prepared = machine
        .create_cpu(relocade_pc::configuration::cpu(), None)
        .unwrap();
    let (_host, endpoint) = wake_connection(prepared.interrupt_handle());
    let runner = Runner::new(clock.clock(), prepared, endpoint)
        .await
        .unwrap();
    runner
        .run(
            futures_timer::Delay::new(Duration::from_secs(30)),
            async |runner| {
                let mut pc = Pc::with_configuration(
                    runner,
                    Configuration {
                        parallel_port: Some(ParallelPort {
                            base: 0x278,
                            irq: 5,
                        }),
                        ..Default::default()
                    },
                )
                .await
                .unwrap();
                let port = pc.parallel_port().unwrap();
                let memory = pc.cpu().memory();
                let io = pc.cpu().context().io.clone();
                memory.view().write(0x1000, &[0xcd, 0x17]).unwrap();
                memory
                    .view()
                    .write(
                        0x1010,
                        &[
                            0xba, 0x78, 2, 0xb0, 0x1b, 0xee, // Data latch = ESC.
                            0xba, 0x7a, 2, 0xb0, 0x0d, 0xee, // Assert STROBE.
                            0xb0, 0x0c, 0xee, // Release STROBE.
                            0xe9, 0xe0, 0xff, // Jump to exit at 1002h.
                        ],
                    )
                    .unwrap();
                let exit = pc.cpu().exit_handle();
                pc.cpu()
                    .install([Hook::host(0x1002, async move || exit.request())])
                    .await
                    .unwrap();

                // An installed adapter with no endpoint reports an empty connector.
                request(&mut pc, 0x025a, 0);
                complete(&mut pc, 0x305a, 0).await;
                memory.view().write_u8(0x478, 0).unwrap();
                request(&mut pc, 0x003f, 0);
                complete(&mut pc, 0x313f, 0).await;
                assert_eq!(io.read_port(0x27a, 1), 0xec);

                let (mut input, _output) = Centronics::connect(pc.runner(), &port).await;
                let mut bytes = Vec::new();
                let controls = Rc::new(RefCell::new(Vec::new()));
                input.on_control({
                    let controls = controls.clone();
                    move |event| controls.borrow_mut().push(event)
                });
                assert_eq!(
                    *controls.borrow(),
                    [ControlEvent::Changed(Controls {
                        select: true,
                        auto_feed: false,
                    })]
                );
                request(&mut pc, 0x025a, 0);
                complete(&mut pc, 0x905a, 0).await;
                input.set_status(Status {
                    ready: false,
                    selected: false,
                    paper_empty: true,
                    error: true,
                });
                request(&mut pc, 0x025a, 0);
                complete(&mut pc, 0x285a, 0).await;
                input.set_status(Status::default());

                // BIOS observes the live table, including a guest-assigned slot.
                memory.view().write_u16(0x40a, 0x278).unwrap();
                request(&mut pc, 0x025a, 1);
                complete(&mut pc, 0x905a, 1).await;
                memory.view().write_u16(0x40a, 0).unwrap();
                for (ax, index) in [
                    (0x005a, 1),
                    (0x015a, 3),
                    (0x025a, 1),
                    (0x025a, 3),
                    (0xff5a, 0),
                ] {
                    request(&mut pc, ax, index);
                    complete(&mut pc, ax, index).await;
                    assert_eq!(io.read_port(0x278, 1), 0x3f);
                    assert_eq!(io.read_port(0x27a, 1), 0xec);
                }
                memory.view().write_u16(0x40a, 0x378).unwrap();
                request(&mut pc, 0x005a, 1);
                complete(&mut pc, 0x005a, 1).await;
                memory.view().write_u16(0x40a, 0).unwrap();

                // Direct guest OUT and BIOS output reach the same byte endpoint.
                request(&mut pc, 0, 0);
                pc.cpu().cpu_state().set_eip(0x1010);
                complete(&mut pc, 0x000c, 0x27a).await;
                bytes.push(read(&mut input).await);
                assert_eq!(bytes, [0x1b]);
                clock.advance_by(Duration::from_micros(5));
                futures_lite::future::yield_now().await;
                request(&mut pc, 0x005a, 0);
                complete(&mut pc, 0x105a, 0).await; // Delivery is pending; no ACK yet.
                bytes.push(read(&mut input).await);
                assert_eq!(bytes, [0x1b, 0x5a]);
                assert_eq!(io.read_port(0x278, 1), 0x5a);
                assert_eq!(io.read_port(0x27a, 1), 0xec);
                request(&mut pc, 0x015a, 0);
                complete(&mut pc, 0x905a, 0).await;
                assert_eq!(
                    controls
                        .borrow()
                        .iter()
                        .filter(|event| **event == ControlEvent::Initialize)
                        .count(),
                    1
                );

                // A live LPT alias waits on the physical adapter. Releasing BUSY
                // completes the send without advancing the clock to a polling tick.
                memory.view().write_u16(0x40a, 0x278).unwrap();
                memory.view().write_u8(0x479, 1).unwrap();
                input.set_status(Status {
                    ready: false,
                    ..Default::default()
                });
                let before_ready = clock.clock().now();
                request(&mut pc, 0x0051, 1);
                pending(&mut pc);
                input.set_status(Status::default());
                complete(&mut pc, 0x1051, 1).await;
                assert_eq!(clock.clock().now(), before_ready);
                bytes.push(read(&mut input).await);
                assert_eq!(bytes, [0x1b, 0x5a, 0x51]);
                memory.view().write_u16(0x40a, 0).unwrap();
                clock.advance_by(Duration::from_micros(5));
                futures_lite::future::yield_now().await;

                // Busy status admits IRQ0 while BIOS waits, then times out without
                // delivering a byte or changing the existing control register.
                input.set_status(Status {
                    ready: false,
                    ..Default::default()
                });
                io.write_port(0x27a, 1, 0x0e);
                memory.view().write_u8(0x478, 1).unwrap();
                let started = clock.clock().now();
                let ticks = memory.view().read_u32(0x46c).unwrap();
                request(&mut pc, 0x003f, 0);
                pending(&mut pc);
                assert!(pc.cpu().cpu_state().interrupts_enabled());
                assert_eq!(io.read_port(0x278, 1), 0x3f);
                clock.advance_by(Duration::from_millis(55));
                futures_lite::future::yield_now().await;
                pending(&mut pc);
                assert!(memory.view().read_u32(0x46c).unwrap() > ticks);
                assert_eq!(io.read_port(0x27a, 1), 0xee);
                clock.advance_to(clock.clock().deadline_after(started, Duration::from_secs(1)));
                complete(&mut pc, 0x113f, 0).await;
                assert_eq!(io.read_port(0x27a, 1), 0xee);
                assert_eq!(bytes, [0x1b, 0x5a, 0x51]);
                memory.view().write_u16(0x408, 0).unwrap();
                request(&mut pc, 0x025a, 0);
                complete(&mut pc, 0x025a, 0).await;
            },
        )
        .await
        .expect("BIOS parallel I/O timed out");
    machine.shutdown().await.unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn bios_uses_the_configured_parallel_ports_and_emulation_clock() {
    pollster::block_on(bios_parallel_io());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn bios_uses_the_configured_parallel_ports_and_emulation_clock() {
    bios_parallel_io().await;
}
