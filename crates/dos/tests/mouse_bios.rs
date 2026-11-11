#![cfg(not(target_arch = "wasm32"))]

use relocade_clock_test_support::ManualClock;
#[path = "mouse_bios/callbacks.rs"]
mod callbacks;
mod common;
#[path = "mouse_bios/integration.rs"]
mod integration;
use relocade_dos::Dos;
use relocade_machine::IoBus;
use relocade_x86::{CpuState, ExecutionExit};

use common::mouse::{wait_status, write};

fn with_mouse(check: impl AsyncFnOnce(&ManualClock, Dos) + Send + 'static) {
    let clock = ManualClock::new();
    let worker_clock = clock.clone();
    let mut machine =
        relocade_machine::Machine::new(relocade_dos::configuration::memory()).unwrap();
    let prepared = machine
        .create_cpu(relocade_dos::configuration::cpu(), None)
        .unwrap();
    let worker = prepared.spawn_with_clock(
        move || worker_clock.clock(),
        async move |runner| {
            let mut dos = Dos::new(runner).await.unwrap();
            dos.launch_com(&[0xcd, 0x15, 0xeb, 0xfe], Default::default())
                .unwrap();
            check(&clock, dos).await;
        },
    );
    assert_eq!(pollster::block_on(worker.join()).unwrap(), Some(()));
    pollster::block_on(machine.shutdown()).unwrap();
}

async fn call(dos: &mut Dos, ax: u16, bx: u16) -> CpuState {
    let state = dos.cpu().cpu_state();
    state.set_eip(0x100);
    state.set_ax(ax);
    state.set_bx(bx);
    state.set_cx(0x1234);
    state.set_dx(0x5678);
    state.set_eflags(0x443); // Saved DF/arithmetic flags and clear IF survive the service.
    assert_eq!(
        common::run_until(dos.cpu(), 0x102).await,
        ExecutionExit::Interrupted
    );
    assert_eq!(state.sp(), 0xfffe);
    assert_eq!(state.eflags() & !1, 0x442);
    state.snapshot()
}

async fn success(dos: &mut Dos, function: u8, bx: u16) -> CpuState {
    let state = call(dos, 0xc200 | u16::from(function), bx).await;
    assert_eq!(state.registers.eax as u16, u16::from(function));
    assert_eq!(state.flags.eflags() & 1, 0);
    state
}

async fn read(io: &IoBus) -> u8 {
    wait_status(io, 0x21, 0x21).await;
    io.read_port(0x60, 1) as u8
}

async fn command(io: &IoBus, byte: u8) {
    write(io, 0x64, 0xd4).await;
    write(io, 0x60, byte).await;
    assert_eq!(read(io).await, 0xfa);
}

#[test]
fn bios_configuration_and_raw_ports_share_one_mouse() {
    with_mouse(async |_, mut dos| {
        let reset = success(&mut dos, 1, 0xffff).await;
        assert_eq!(reset.registers.ebx as u16, 0x00aa);
        success(&mut dos, 2, 0x0600).await; // 200 samples/s.
        success(&mut dos, 3, 0x0300).await; // 8 counts/mm.
        success(&mut dos, 6, 0x0200).await; // 2:1 scaling.
        let io = dos.cpu().context().io.clone();
        write(&io, 0x64, 0xa8).await;
        command(&io, 0xe9).await;
        assert_eq!(
            [read(&io).await, read(&io).await, read(&io).await],
            [0x10, 3, 200]
        );
        command(&io, 0xf3).await;
        command(&io, 40).await;
        command(&io, 0xe8).await;
        command(&io, 0).await;
        let status = success(&mut dos, 6, 0x0055).await;
        assert_eq!(status.registers.ebx as u16, 0x0010);
        assert_eq!(status.registers.ecx as u16, 0x1200);
        assert_eq!(status.registers.edx as u16, 0x5628);
        let id = success(&mut dos, 4, 0xffff).await;
        assert_eq!(id.registers.ebx as u16, 0x00ff);
        success(&mut dos, 5, 0x0300).await;
        let status = success(&mut dos, 6, 0).await;
        assert_eq!(status.registers.ebx as u16, 0);
        assert_eq!(status.registers.ecx as u16, 0x1202);
        assert_eq!(status.registers.edx as u16, 0x5664);
        success(&mut dos, 0, 0).await;
    });
}

#[test]
fn bios_mouse_errors_preserve_saved_flags_and_configuration() {
    with_mouse(async |_, mut dos| {
        for (ax, bx, error) in [
            (0xc200, 0x0100, 5), // No callback installed.
            (0xc200, 0x0200, 2),
            (0xc202, 0x0700, 2),
            (0xc203, 0x0400, 2),
            (0xc205, 0x0000, 2),
            (0xc205, 0x0400, 3), // Only the standard three-byte packet is supported.
            (0xc206, 0x0300, 2),
            (0xc208, 0x0000, 1),
            (0x8800, 0x0000, 0x86),
        ] {
            let state = call(&mut dos, ax, bx).await;
            assert_eq!(state.registers.eax as u16, (ax & 0xff) | (error << 8));
            assert_eq!(state.flags.eflags(), 0x443);
            assert_eq!(state.registers.ebx as u16, bx);
            assert_eq!(state.registers.ecx as u16, 0x1234);
            assert_eq!(state.registers.edx as u16, 0x5678);
        }
        let status = success(&mut dos, 6, 0).await;
        assert_eq!(status.registers.ebx as u16, 0);
        assert_eq!(status.registers.ecx as u16, 0x1202);
        assert_eq!(status.registers.edx as u16, 0x5664);
    });
}
