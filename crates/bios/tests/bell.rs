#![cfg(not(target_arch = "wasm32"))]

#[path = "../../machine/tests/support/clock.rs"]
mod clock;

use clock::ManualClock;
use relocade_bios::Teletype;
use relocade_machine::{DeviceConnection, IoBus, MachineConfig, Memory, MemoryBus};
use relocade_pc_speaker::{Speaker, SPEAKER_CONTROL_PORT};
use relocade_pit::{CHANNEL_2_PORT, CONTROL_PORT};
use relocade_x86::{PhysicalMapping, PhysicalMemoryMap};
use std::{
    cell::RefCell,
    future::Future,
    pin::pin,
    rc::Rc,
    task::{Context, Waker},
    time::Duration,
};

struct Fixture {
    clock: ManualClock,
    memory: MemoryBus,
    teletype: Teletype<ManualClock>,
}

impl Fixture {
    fn new() -> Self {
        let clock = ManualClock::default();
        let clock_handle = Rc::new(clock.clone());
        let io = IoBus::new([
            DeviceConnection {
                device: Rc::new(RefCell::new(relocade_vga::Vga::with_clock(
                    clock_handle.clone(),
                ))),
                mmio: vec![relocade_vga::MEMORY_RANGE],
                ports: relocade_vga::PORT_RANGES.into_iter().collect(),
            },
            DeviceConnection {
                device: Rc::new(RefCell::new(Speaker::with_clock(clock_handle.clone()))),
                mmio: vec![],
                ports: relocade_pc_speaker::PORTS.into_iter().collect(),
            },
        ]);
        let map = PhysicalMemoryMap::new([
            (0..=0x9ffff, PhysicalMapping::Ram { backing_offset: 0 }),
            (relocade_vga::MEMORY_RANGE, PhysicalMapping::Mmio),
            (
                0xf0000..=0xfffff,
                PhysicalMapping::Rom {
                    backing_offset: 0xf0000,
                },
            ),
        ])
        .unwrap();
        let memory = Memory::new(MachineConfig::new(0x100000, map).unwrap())
            .unwrap()
            .with_io(io);
        relocade_bios::initialize(&mut memory.view(), memory.io());
        let teletype = Teletype::new(memory.clone(), clock_handle);
        Self {
            clock,
            memory,
            teletype,
        }
    }

    fn enabled(&self) -> u32 {
        self.memory.io().read_port(SPEAKER_CONTROL_PORT, 1) & 3
    }
}

#[test]
fn bell_programs_channel_two_and_finishes_after_100ms_without_moving_the_cursor() {
    let fixture = Fixture::new();
    let io = fixture.memory.io();
    // BEL replaces an already enabled speaker and leaves it disabled afterward.
    io.write_port(SPEAKER_CONTROL_PORT, 1, 3);
    let mut write = pin!(fixture.teletype.write_text(b"A\x07B"));
    let mut cx = Context::from_waker(Waker::noop());
    assert!(write.as_mut().poll(&mut cx).is_pending());
    assert_eq!(fixture.enabled(), 3);
    assert_eq!(fixture.memory.view().read_u16(0x450).unwrap(), 1);
    assert_eq!(fixture.memory.view().read_u16(0xb8000).unwrap(), 0x0741);
    assert_eq!(fixture.memory.view().read_u16(0xb8002).unwrap(), 0x0720);

    // One PIT input tick loads the divisor. In mode 3 an odd count reads even.
    fixture.clock.advance(Duration::from_micros(1));
    io.write_port(CONTROL_PORT, 1, 0xc8); // Latch channel-2 status and count.
    assert_eq!(io.read_port(CHANNEL_2_PORT, 1) & 0x7f, 0x36);
    let low = io.read_port(CHANNEL_2_PORT, 1) as u8;
    let high = io.read_port(CHANNEL_2_PORT, 1) as u8;
    assert_eq!(u16::from_le_bytes([low, high]), 1192);
    fixture.clock.advance(Duration::from_millis(99));
    assert!(write.as_mut().poll(&mut cx).is_pending());
    assert_eq!(fixture.enabled(), 3);
    fixture.clock.advance(Duration::from_millis(100));
    assert!(write.as_mut().poll(&mut cx).is_ready());
    assert_eq!(fixture.enabled(), 0);
    assert_eq!(fixture.memory.view().read_u16(0xb8002).unwrap(), 0x0742);
    assert_eq!(fixture.memory.view().read_u16(0x450).unwrap(), 2);
}

#[test]
fn cancelling_output_stops_the_bell_and_keeps_the_completed_text_prefix() {
    let fixture = Fixture::new();
    let mut write = Box::pin(fixture.teletype.write_text(b"A\x07B"));
    assert!(write
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
        .is_pending());
    assert_eq!(fixture.enabled(), 3);
    drop(write);
    assert_eq!(fixture.enabled(), 0);
    assert_eq!(fixture.memory.view().read_u16(0x450).unwrap(), 1);
    assert_eq!(fixture.memory.view().read_u16(0xb8000).unwrap(), 0x0741);
    assert_eq!(fixture.memory.view().read_u16(0xb8002).unwrap(), 0x0720);
    pollster::block_on(fixture.teletype.write_text(b"C"));
    assert_eq!(fixture.memory.view().read_u16(0xb8002).unwrap(), 0x0743);
}
