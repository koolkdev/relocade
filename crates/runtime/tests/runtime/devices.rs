use super::{ram, runtime};
use wasm86_runtime::{DeviceMemory, Devices, StepOutcome};
use wasm86_x86::{
    PhysicalMapping::{Mmio, Ram},
    PhysicalMemoryMap, Segment, StoredSegment,
};

#[derive(Default)]
struct Bus {
    events: Vec<(char, u32, u8, u64)>,
}

impl Devices for Bus {
    fn read_port(&mut self, _: &mut DeviceMemory<'_>, port: u16, bytes: u8) -> u32 {
        self.events.push(('i', u32::from(port), bytes, 0));
        0xabcd_1234
    }
    fn write_port(&mut self, _: &mut DeviceMemory<'_>, port: u16, bytes: u8, value: u32) {
        self.events
            .push(('o', u32::from(port), bytes, u64::from(value)));
    }
    fn read_mmio(&mut self, _: &mut DeviceMemory<'_>, address: u32, bytes: u8) -> u64 {
        self.events.push(('r', address, bytes, 0));
        0xfedc_ba98
    }
    fn write_mmio(&mut self, _: &mut DeviceMemory<'_>, address: u32, bytes: u8, value: u64) {
        self.events.push(('w', address, bytes, value));
    }
}

#[test]
fn device_callbacks_preserve_cpu_operand_widths() {
    let map = PhysicalMemoryMap::new([
        (0..=0xffff, Ram { backing_offset: 0 }),
        (0x2000..=0x2fff, Mmio),
    ])
    .unwrap();
    let mut machine = runtime().machine(map, Bus::default()).unwrap();
    machine
        .write_memory(
            0x100,
            &[0xed, 0x66, 0xef, 0x66, 0xa1, 0, 0x20, 0x66, 0xa3, 0, 0x20],
        )
        .unwrap();
    let mut cpu = machine.cpu();
    cpu.eip = 0x100;
    cpu.registers.eax = 0x9876_0000;
    cpu.registers.edx = 0xffff;
    machine.set_cpu(cpu).unwrap();
    for _ in 0..4 {
        assert_eq!(machine.step().unwrap(), StepOutcome::Completed);
    }
    assert_eq!(machine.cpu().registers.eax, 0xfedc_ba98);
    assert_eq!(
        machine.devices().events,
        [
            ('i', 0xffff, 2, 0),
            ('o', 0xffff, 4, 0x9876_1234),
            ('r', 0x2000, 4, 0),
            ('w', 0x2000, 4, 0xfedc_ba98),
        ]
    );
}

struct A20;
impl Devices for A20 {
    fn write_port(&mut self, memory: &mut DeviceMemory<'_>, port: u16, _: u8, value: u32) {
        assert_eq!(port, 0x92);
        memory.set_a20_enabled(value & 2 != 0);
    }
}

#[test]
fn port_mapping_changes_are_installed_before_the_next_entry() {
    let mut machine = runtime().machine(ram(), A20).unwrap();
    machine
        .write_memory(0x100, &[0xe6, 0x92, 0xa0, 0x10, 0])
        .unwrap();
    machine.with_memory(|memory| memory.backing_mut()[0x100000] = 0x42);
    let mut cpu = machine.cpu();
    cpu.eip = 0x100;
    cpu.registers.eax = 2;
    cpu.segments.ds = StoredSegment::real_mode(Segment::Ds, 0xffff);
    machine.set_cpu(cpu).unwrap();
    assert_eq!(machine.step().unwrap(), StepOutcome::Completed);
    assert_eq!(machine.step().unwrap(), StepOutcome::Completed);
    assert_eq!(machine.cpu().registers.eax, 0x42);
}

struct Remap;
impl Devices for Remap {
    fn read_mmio(&mut self, memory: &mut DeviceMemory<'_>, address: u32, bytes: u8) -> u64 {
        assert_eq!((address, bytes), (0x1fff, 1));
        memory.backing_mut()[0x5000] = 0x12;
        memory
            .set_map(
                PhysicalMemoryMap::new([
                    (0..=0xffff, Ram { backing_offset: 0 }),
                    (0x1000..=0x1fff, Mmio),
                    (
                        0x2000..=0x2fff,
                        Ram {
                            backing_offset: 0x5000,
                        },
                    ),
                ])
                .unwrap(),
            )
            .unwrap();
        0x34
    }
}

#[test]
fn mmio_remapping_affects_the_remaining_part_of_one_operand() {
    let map = PhysicalMemoryMap::new([
        (0..=0xffff, Ram { backing_offset: 0 }),
        (0x1000..=0x1fff, Mmio),
    ])
    .unwrap();
    let mut machine = runtime().machine(map, Remap).unwrap();
    machine.write_memory(0x100, &[0xa1, 0xff, 0x1f]).unwrap();
    let mut cpu = machine.cpu();
    cpu.eip = 0x100;
    machine.set_cpu(cpu).unwrap();
    assert_eq!(machine.step().unwrap(), StepOutcome::Completed);
    assert_eq!(machine.cpu().registers.eax, 0x1234);
}
