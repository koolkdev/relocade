use super::{ram, runtime};
use wasm86_machine::{Environment, MemoryAccess};
use wasm86_x86::{
    ExecutionExit,
    PhysicalMapping::{Mmio, Ram},
    PhysicalMemoryMap, Segment, StoredSegment,
};

#[derive(Default)]
struct Bus {
    events: Vec<(char, u32, u8, u64)>,
}

impl Environment for Bus {
    fn read_port(&mut self, _: &mut MemoryAccess<'_>, port: u16, bytes: u8) -> u32 {
        self.events.push(('i', u32::from(port), bytes, 0));
        0xabcd_1234
    }
    fn write_port(&mut self, _: &mut MemoryAccess<'_>, port: u16, bytes: u8, value: u32) {
        self.events
            .push(('o', u32::from(port), bytes, u64::from(value)));
    }
    fn read_mmio(&mut self, _: &mut MemoryAccess<'_>, address: u32, bytes: u8) -> u64 {
        self.events.push(('r', address, bytes, 0));
        0xfedc_ba98
    }
    fn write_mmio(&mut self, _: &mut MemoryAccess<'_>, address: u32, bytes: u8, value: u64) {
        self.events.push(('w', address, bytes, value));
    }
}

#[cfg_attr(not(target_arch = "wasm32"), pollster::test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
async fn device_callbacks_preserve_cpu_operand_widths() {
    let map = PhysicalMemoryMap::new([
        (0..=0xffff, Ram { backing_offset: 0 }),
        (0x2000..=0x2fff, Mmio),
    ])
    .unwrap();
    let mut machine = runtime()
        .await
        .attach(super::real_machine(map, Bus::default()))
        .await
        .unwrap();
    machine
        .write_memory(
            0x100,
            &[
                0xed, 0x66, 0xef, 0x66, 0xa1, 0, 0x20, 0x66, 0xa3, 0, 0x20, 0xeb, 0,
            ],
        )
        .unwrap();
    let mut cpu = machine.cpu();
    cpu.eip = 0x100;
    cpu.registers.eax = 0x9876_0000;
    cpu.registers.edx = 0xffff;
    machine.set_cpu(cpu).unwrap();
    for _ in 0..3 {
        assert_eq!(machine.run(1).unwrap(), ExecutionExit::BudgetExhausted);
    }
    assert_eq!(machine.cpu().registers.eax, 0xfedc_ba98);
    assert_eq!(
        machine.with_environment(|devices| devices.events.clone()),
        [
            ('i', 0xffff, 2, 0),
            ('o', 0xffff, 4, 0x9876_1234),
            ('r', 0x2000, 4, 0),
            ('w', 0x2000, 4, 0xfedc_ba98),
        ]
    );
}

struct A20;
impl Environment for A20 {
    fn write_port(&mut self, memory: &mut MemoryAccess<'_>, port: u16, _: u8, value: u32) {
        assert_eq!(port, 0x92);
        memory.set_a20_enabled(value & 2 != 0).unwrap();
    }
}

#[cfg_attr(not(target_arch = "wasm32"), pollster::test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
async fn port_mapping_changes_are_installed_before_the_next_entry() {
    let mut machine = runtime()
        .await
        .attach(super::real_machine(ram(), A20))
        .await
        .unwrap();
    machine
        .write_memory(0x100, &[0xe6, 0x92, 0xa0, 0x10, 0, 0xeb, 0])
        .unwrap();
    machine.with_memory(|memory| memory.write_backing(0x100000, &[0x42]).unwrap());
    let mut cpu = machine.cpu();
    cpu.eip = 0x100;
    cpu.registers.eax = 2;
    cpu.segments.ds = StoredSegment::real_mode(Segment::Ds, 0xffff);
    machine.set_cpu(cpu).unwrap();
    assert_eq!(machine.run(1).unwrap(), ExecutionExit::BudgetExhausted);
    assert_eq!(machine.run(1).unwrap(), ExecutionExit::BudgetExhausted);
    assert_eq!(machine.cpu().registers.eax, 0x42);
}

struct Remap;
impl Environment for Remap {
    fn read_mmio(&mut self, memory: &mut MemoryAccess<'_>, address: u32, bytes: u8) -> u64 {
        assert_eq!((address, bytes), (0x1fff, 1));
        memory.write_backing(0x5000, &[0x12]).unwrap();
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

#[cfg_attr(not(target_arch = "wasm32"), pollster::test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
async fn mmio_remapping_affects_the_remaining_part_of_one_operand() {
    let map = PhysicalMemoryMap::new([
        (0..=0xffff, Ram { backing_offset: 0 }),
        (0x1000..=0x1fff, Mmio),
    ])
    .unwrap();
    let mut machine = runtime()
        .await
        .attach(super::real_machine(map, Remap))
        .await
        .unwrap();
    machine
        .write_memory(0x100, &[0xa1, 0xff, 0x1f, 0xeb, 0])
        .unwrap();
    let mut cpu = machine.cpu();
    cpu.eip = 0x100;
    machine.set_cpu(cpu).unwrap();
    assert_eq!(machine.run(1).unwrap(), ExecutionExit::BudgetExhausted);
    assert_eq!(machine.cpu().registers.eax, 0x1234);
}

#[cfg_attr(not(target_arch = "wasm32"), pollster::test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
async fn local_devices_preserve_all_64_mmio_bits() {
    use std::{cell::RefCell, rc::Rc};
    const VALUE: u64 = 0xfedc_ba98_7654_3210;
    struct WideBus(Rc<RefCell<Vec<u64>>>);
    impl Environment for WideBus {
        fn read_mmio(&mut self, _: &mut MemoryAccess<'_>, address: u32, bytes: u8) -> u64 {
            assert_eq!((address, bytes), (0x2000, 8));
            VALUE
        }
        fn write_mmio(&mut self, _: &mut MemoryAccess<'_>, address: u32, bytes: u8, value: u64) {
            assert_eq!((address, bytes), (0x2008, 8));
            self.0.borrow_mut().push(value);
        }
    }
    let events = Rc::new(RefCell::new(Vec::new()));
    let map = PhysicalMemoryMap::new([
        (0..=0xffff, Ram { backing_offset: 0 }),
        (0x2000..=0x2fff, Mmio),
    ])
    .unwrap();
    let mut machine = runtime()
        .await
        .attach(super::real_machine(map, WideBus(events.clone())))
        .await
        .unwrap();
    // MOVQ MM0,[2000]; MOVQ [2008],MM0.
    machine
        .write_memory(
            0x100,
            &[
                0x0f, 0x6f, 0x06, 0, 0x20, 0x0f, 0x7f, 0x06, 8, 0x20, 0xeb, 0,
            ],
        )
        .unwrap();
    let mut cpu = machine.cpu();
    cpu.eip = 0x100;
    machine.set_cpu(cpu).unwrap();
    assert_eq!(machine.run(1).unwrap(), ExecutionExit::BudgetExhausted);
    assert_eq!(&*events.borrow(), &[VALUE]);
    machine.with_environment_mut(|device| device.0.borrow_mut().clear());
    assert!(machine.with_environment(|device| device.0.borrow().is_empty()));
}
