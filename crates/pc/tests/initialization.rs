use relocade_clock_test_support::ManualClock;
use relocade_frontend::input::InputEvent;
use relocade_frontend::keyboard::{ElementState, Key, KeyCode, KeyLocation, KeyboardEvent};
use relocade_machine::{
    real_mode_interrupt::{read_frame, read_vector},
    worker::wake_connection,
    FarPointer, Hook, Runner,
};
use relocade_pc::Pc;
use relocade_x86::{Exception, ExecutionExit, HostExit, Segment, SegmentRegister};
use std::time::Duration;

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

async fn bios_without_dos() {
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
                let mut pc = Pc::new(runner).await.unwrap();
                let memory = pc.cpu().memory();
                // PC startup installs neither a DOS vector nor DOS allocation metadata.
                assert_eq!(memory.view().read_u32(0x21 * 4).unwrap(), 0);
                assert_eq!(memory.view().read_u8(0xfff0).unwrap(), 0);
                assert_eq!(memory.view().read_u32(0x46c).unwrap(), 0);

                // AdLib detection works on the initialized PC without an audio sink.
                let io = pc.cpu().context().io.clone();
                assert_eq!(io.read_port(0x388, 1), 0x06);
                io.write_port(0x388, 1, 2);
                io.write_port(0x389, 1, 0xff);
                io.write_port(0x388, 1, 4);
                io.write_port(0x389, 1, 0x21);
                clock.advance_by(Duration::from_micros(80));
                assert_eq!(io.read_port(0x388, 1), 0xc6);
                io.write_port(0x389, 1, 0x60);
                assert_eq!(io.read_port(0x388, 1), 0x06);

                // A peripheral uses the same DMA registers the guest programs.
                use relocade_dma::{
                    ADDRESS_PORTS, CLEAR_FLIP_FLOP_PORT, COUNT_PORTS, MODE_PORT, PAGE_PORTS,
                    SINGLE_MASK_PORT, STATUS_PORT,
                };
                memory.view().write(0x20000, &[0x80, 0xa0]).unwrap();
                for (port, value) in [
                    (SINGLE_MASK_PORT, 5),
                    (CLEAR_FLIP_FLOP_PORT, 0),
                    (ADDRESS_PORTS[1], 0),
                    (ADDRESS_PORTS[1], 0),
                    (COUNT_PORTS[1], 1),
                    (COUNT_PORTS[1], 0),
                    (PAGE_PORTS[1], 2),
                    (MODE_PORT, 0x59), // Auto-initialize, single memory read, channel 1.
                    (SINGLE_MASK_PORT, 1),
                ] {
                    io.write_port(port, 1, value);
                }
                let dma = pc.dma();
                let view = memory.view().with_io(&io);
                let mut samples = [0; 3];
                assert_eq!(dma.borrow_mut().read(1, &view, &mut samples), 3);
                assert_eq!(samples, [0x80, 0xa0, 0x80]);
                assert_eq!(io.read_port(STATUS_PORT, 1), 2);

                // Query the default card's video firmware, then physical input and IRQ0.
                let code = [
                    0xb4, 0x0f, 0xcd, 0x10, 0xa3, 0, 7, // Video mode.
                    0xbf, 0, 8, 0xb8, 0, 0x4f, 0xcd, 0x10, 0xa3, 6, 7, // VBE info.
                    0x31, 0xc0, 0xcd, 0x16, 0xa3, 2, 7, // Key.
                    0x83, 0x3e, 0x6c, 4, 0, 0x74, 0xf9, // Wait for IRQ0's BDA update.
                    0x31, 0xc0, 0xcd, 0x1a, 0x89, 0x16, 4, 7, // Clock.
                    0xeb, 0,
                ];
                memory.view().write(0x1000, &code).unwrap();
                let exit = pc.cpu().exit_handle();
                pc.cpu()
                    .install([Hook::host(0x1000 + code.len() as u32, async move || {
                        exit.request();
                    })])
                    .await
                    .unwrap();
                let state = pc.cpu().cpu_state();
                state.set_eip(0x1000);
                state.set_sp(0x8000);
                state.set_interrupts_enabled(true);
                pc.keyboard_input()
                    .unbounded_send(InputEvent::new(KeyboardEvent::Key {
                        physical_key: KeyCode::KeyA.into(),
                        logical_key: Key::Character("a".into()),
                        location: KeyLocation::Standard,
                        text: Some("a".into()),
                        state: ElementState::Pressed,
                        repeat: false,
                    }))
                    .unwrap();
                clock.advance_to(clock.at(Duration::from_nanos(54_925_402)));
                assert_eq!(pc.run().await, ExecutionExit::Host(HostExit::Return));
                assert_eq!(memory.view().read_u16(0x700).unwrap(), 0x5003);
                assert_eq!(memory.view().read_u16(0x702).unwrap(), 0x1e61);
                assert_eq!(memory.view().read_u16(0x704).unwrap(), 1);
                assert_eq!(memory.view().read_u16(0x706).unwrap(), 0x004f);
                assert_eq!(
                    memory.view().read_u32(0x800).unwrap(),
                    u32::from_le_bytes(*b"VESA")
                );
                assert_eq!(memory.view().read_u32(0x21 * 4).unwrap(), 0);
            },
        )
        .await
        .expect("PC guest timed out");
    machine.shutdown().await.unwrap();
}

async fn default_exceptions() {
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
                let mut pc = Pc::new(runner).await.unwrap();
                let memory = pc.cpu().memory();
                for (code, exception) in [
                    (&[0xf6, 0xf3][..], Exception::DivideError), // DIV BL.
                    (&[0x62, 0x06, 0, 5][..], Exception::BoundRangeExceeded), // BOUND AX,[0500].
                    (&[0x0f, 0x0b][..], Exception::InvalidOpcode), // UD2.
                ] {
                    memory.view().write(0x1000, code).unwrap();
                    let state = pc.cpu().cpu_state();
                    state.set_segment(Segment::Cs, SegmentRegister::real_mode(Segment::Cs, 0));
                    state.set_eip(0x1000);
                    state.set_sp(0x8000);
                    state.set_ax(1);
                    state.set_bx(0);
                    state.set_eflags(2);
                    let handler = read_vector(&memory.view(), exception.vector() as u8);
                    assert_eq!(handler.segment, 0xf000, "default entry is in BIOS ROM");
                    assert_eq!(pc.run().await, ExecutionExit::UnhandledException(exception));
                    assert_eq!(state.segment(Segment::Cs).selector, handler.segment);
                    let frame = read_frame(&state, &memory.view());
                    assert_eq!(
                        frame.return_address,
                        FarPointer {
                            segment: 0,
                            offset: 0x1000
                        }
                    );
                    assert_eq!(frame.flags, 2);
                }
            },
        )
        .await
        .expect("PC guest timed out");
    machine.shutdown().await.unwrap();
}

async fn absent_services() {
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
                let mut pc = Pc::new(runner).await.unwrap();
                let memory = pc.cpu().memory();
                let vectors = [0x14, 0x17];
                let mut code = Vec::new();
                for vector in vectors {
                    code.extend_from_slice(&[0xcd, vector]);
                }
                code.extend_from_slice(&[0xcd, 0x1b, 0xcd, 0x1c]);
                memory.view().write(0x1000, &code).unwrap();
                let exit = pc.cpu().exit_handle();
                pc.cpu()
                    .install([Hook::host(0x1000 + code.len() as u32, async move || {
                        exit.request()
                    })])
                    .await
                    .unwrap();
                let state = pc.cpu().cpu_state();
                state.set_eip(0x1000);
                state.set_sp(0x8000);
                state.set_eax(0x1234_5678);
                state.set_ebx(0x5678_abcd);
                state.set_ecx(0x1357_2468);
                state.set_edx(0xfedc_ba98);
                state.set_eflags(0xa57);
                let before = state.snapshot();
                assert_eq!(pc.run().await, ExecutionExit::Host(HostExit::Return));
                assert_eq!(state.snapshot().registers, before.registers);
                assert_eq!(state.snapshot().segments, before.segments);
                assert_eq!(state.eflags(), 0xa57);
                for vector in [0x21, 0x33] {
                    assert_eq!(
                        memory.view().read_u32(vector * 4).unwrap(),
                        0,
                        "DOS is not installed"
                    );
                }
                for vector in [0x1f, 0x43] {
                    assert_ne!(
                        read_vector(&memory.view(), vector),
                        read_vector(&memory.view(), 0x14),
                        "font data is not a callable default"
                    );
                }
            },
        )
        .await
        .expect("PC guest timed out");
    machine.shutdown().await.unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn pc_initializes_hardware_and_bios_without_dos() {
    pollster::block_on(bios_without_dos());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn pc_initializes_hardware_and_bios_without_dos() {
    bios_without_dos().await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn pc_installs_default_exception_handlers() {
    pollster::block_on(default_exceptions());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn pc_installs_default_exception_handlers() {
    default_exceptions().await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn pc_returns_from_absent_services_and_default_callbacks() {
    pollster::block_on(absent_services());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn pc_returns_from_absent_services_and_default_callbacks() {
    absent_services().await;
}
