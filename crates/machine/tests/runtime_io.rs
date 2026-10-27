mod support;

use relocade_machine::{Cpu, Device, DeviceConnection, DispatchEntry, IoBus, Memory};
use relocade_x86::{
    BlockSnapshot, Compiler, CpuState, ExecutionExit, ExecutionProfile, PhysicalMapping,
    PhysicalMemoryMap, Segment, SegmentRegister, Segments,
};
use std::{cell::RefCell, rc::Rc};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[derive(Debug, PartialEq)]
enum Transfer {
    MemoryRead { address: u32, bytes: u8 },
    MemoryWrite { address: u32, bytes: u8, value: u64 },
    PortRead { port: u16, bytes: u8 },
    PortWrite { port: u16, bytes: u8, value: u32 },
}

struct RecordingDevice {
    base: u32,
    memory: Vec<u8>,
    port: u32,
    transfers: Vec<Transfer>,
}

impl RecordingDevice {
    fn new(base: u32) -> Rc<RefCell<Self>> {
        Rc::new(RefCell::new(Self {
            base,
            memory: vec![0; 0x2000],
            port: 0,
            transfers: Vec::new(),
        }))
    }

    fn take_transfers(&mut self) -> Vec<Transfer> {
        std::mem::take(&mut self.transfers)
    }
}

impl Device for RecordingDevice {
    fn read_mmio(&mut self, address: u32, bytes: u8) -> u64 {
        self.transfers.push(Transfer::MemoryRead { address, bytes });
        let offset = (address - self.base) as usize;
        let mut result = [0; 8];
        result[..usize::from(bytes)]
            .copy_from_slice(&self.memory[offset..offset + usize::from(bytes)]);
        u64::from_le_bytes(result)
    }

    fn write_mmio(&mut self, address: u32, bytes: u8, value: u64) {
        self.transfers.push(Transfer::MemoryWrite {
            address,
            bytes,
            value,
        });
        let offset = (address - self.base) as usize;
        self.memory[offset..offset + usize::from(bytes)]
            .copy_from_slice(&value.to_le_bytes()[..usize::from(bytes)]);
    }

    fn read_port(&mut self, port: u16, bytes: u8) -> u32 {
        self.transfers.push(Transfer::PortRead { port, bytes });
        self.port
    }

    fn write_port(&mut self, port: u16, bytes: u8, value: u32) {
        self.transfers
            .push(Transfer::PortWrite { port, bytes, value });
        self.port = value;
    }
}

fn map() -> PhysicalMemoryMap {
    PhysicalMemoryMap::new([
        (0..=0x1fff, PhysicalMapping::Ram { backing_offset: 0 }),
        (0x3000..=0x4fff, PhysicalMapping::Mmio),
    ])
    .unwrap()
}

async fn check_device_access() {
    // Both entries must bind the same I/O bus; neither may bypass it with RAM.
    for compiled in [false, true] {
        let device = RecordingDevice::new(0x3000);
        let io = IoBus::new([DeviceConnection {
            device: device.clone(),
            mmio: vec![0x3000..=0x4fff],
            ports: vec![0x3d4..=0x3d4],
        }]);
        let compiler = Compiler::new(ExecutionProfile::Real16);
        let mut segments = Segments::real_mode();
        segments.ds = SegmentRegister::real_mode(Segment::Ds, 0xffff);
        let config = support::configuration(
            compiler.target(),
            CpuState {
                eip: 0x100,
                segments,
                ..CpuState::default()
            },
            0x5000,
            map(),
        )
        .unwrap();
        let memory = Memory::new(config.machine).unwrap();
        let mut view = memory.view().with_io(&io);
        view.write_backing(0x3ffe, &[0xaa; 4]).unwrap();
        // Adjacent MMIO pages preserve the transfer width after A20 translation.
        view.write(0x103ffe, &[1, 2, 3]).unwrap();
        assert_eq!(view.read_u32(0x3ffe).unwrap(), 0x0003_0201);
        assert_eq!(
            device.borrow_mut().take_transfers(),
            [
                Transfer::MemoryWrite {
                    address: 0x3ffe,
                    bytes: 3,
                    value: 0x030201
                },
                Transfer::MemoryRead {
                    address: 0x3ffe,
                    bytes: 4
                },
            ]
        );

        let code = [
            // DS:400E crosses two MMIO pages after A20 translation.
            0x66, 0xb8, 0x78, 0x56, 0x34, 0x12, // MOV EAX,12345678
            0x66, 0xa3, 0x0e, 0x40, // MOV [400E],EAX
            0x66, 0xa1, 0x0e, 0x40, // MOV EAX,[400E]
            0xba, 0xd4, 0x03, // MOV DX,03D4
            0x66, 0xef, // OUT DX,EAX: one transfer, despite the one-port mapping.
            0x66, 0xed, // IN EAX,DX
            0xeb, 0, // JMP to the next entry boundary.
        ];
        view.write(0x100, &code).unwrap();
        let mut cpu = Cpu::new(memory.clone(), config.cpu, io.clone(), ())
            .await
            .unwrap();
        if compiled {
            // OUT and IN each end a block; install all three captured entries.
            let snapshots = [0, 19, 21].map(|offset| BlockSnapshot {
                start_eip: 0x100 + offset as u32,
                bytes: &code[offset..],
                instruction_limit: 7,
            });
            let module = compiler.compile_blocks(&snapshots).unwrap();
            let entries: Vec<_> = module
                .entries
                .iter()
                .map(|entry| DispatchEntry {
                    address: entry.start_eip,
                    export: &entry.export,
                })
                .collect();
            cpu.install_module(&module.bytes, &entries).await.unwrap();
        }
        assert_eq!(cpu.run(7).await, ExecutionExit::BudgetExhausted);
        assert_eq!(cpu.context().cpu.eax(), 0x1234_5678);
        assert_eq!(
            device.borrow_mut().take_transfers(),
            [
                Transfer::MemoryWrite {
                    address: 0x3ffe,
                    bytes: 4,
                    value: 0x1234_5678
                },
                Transfer::MemoryRead {
                    address: 0x3ffe,
                    bytes: 4
                },
                Transfer::PortWrite {
                    port: 0x3d4,
                    bytes: 4,
                    value: 0x1234_5678
                },
                Transfer::PortRead {
                    port: 0x3d4,
                    bytes: 4
                },
            ]
        );
        assert_eq!(view.read_u32(0x103ffe).unwrap(), 0x1234_5678);
        let mut backing = [0; 4];
        view.read_backing(0x3ffe, &mut backing).unwrap();
        assert_eq!(backing, [0xaa; 4]);

        cpu.context().io.write_port(0x3d4, 2, 0x9876);
        assert_eq!(cpu.context().io.read_port(0x3d4, 2), 0x9876);
        assert_eq!(cpu.context().io.read_port(0x3d5, 1), 0xff);
        // The local bus, rather than shared RAM, retains the device after CPU teardown.
        drop(cpu);
        assert_eq!(view.read_u32(0x3ffe).unwrap(), 0x1234_5678);
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn host_and_guest_access_the_same_devices() {
    pollster::block_on(check_device_access());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn host_and_guest_access_the_same_devices() {
    check_device_access().await;
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn mmio_crosses_device_boundaries_and_open_bus() {
    let first = RecordingDevice::new(0x3000);
    let second = RecordingDevice::new(0x3000);
    let io = IoBus::new([
        DeviceConnection {
            device: first.clone(),
            mmio: vec![0x3000..=0x3002],
            ports: vec![],
        },
        DeviceConnection {
            device: second.clone(),
            mmio: vec![0x3004..=0x3fff],
            ports: vec![],
        },
    ]);
    let memory = Memory::new(relocade_machine::MachineConfig::new(0x2000, map()).unwrap()).unwrap();
    let mut view = memory.view().with_io(&io);
    view.write_u64(0x3000, 0x0807_0605_0403_0201).unwrap();
    assert_eq!(view.read_u64(0x3000).unwrap(), 0x0807_0605_ff03_0201);
    assert_eq!(
        first.borrow_mut().take_transfers(),
        [
            Transfer::MemoryWrite {
                address: 0x3000,
                bytes: 3,
                value: 0x030201
            },
            Transfer::MemoryRead {
                address: 0x3000,
                bytes: 3
            },
        ]
    );
    assert_eq!(
        second.borrow_mut().take_transfers(),
        [
            Transfer::MemoryWrite {
                address: 0x3004,
                bytes: 4,
                value: 0x0807_0605
            },
            Transfer::MemoryRead {
                address: 0x3004,
                bytes: 4
            },
        ]
    );
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn a20_wrap_splits_mmio_at_the_bus_address_discontinuity() {
    let low = RecordingDevice::new(0);
    let high = RecordingDevice::new(0xff000);
    let io = IoBus::new([
        DeviceConnection {
            device: low.clone(),
            mmio: vec![0..=0xfff],
            ports: vec![],
        },
        DeviceConnection {
            device: high.clone(),
            mmio: vec![0xff000..=0xfffff],
            ports: vec![],
        },
    ]);
    let map = PhysicalMemoryMap::new([
        (0..=0xfff, PhysicalMapping::Mmio),
        (0xff000..=0xfffff, PhysicalMapping::Mmio),
    ])
    .unwrap();
    let memory = Memory::new(relocade_machine::MachineConfig::new(0, map).unwrap()).unwrap();
    let mut view = memory.view().with_io(&io);
    view.write_u32(0xffffe, 0x4433_2211).unwrap();
    assert_eq!(view.read_u32(0xffffe).unwrap(), 0x4433_2211);
    assert_eq!(
        high.borrow_mut().take_transfers(),
        [
            Transfer::MemoryWrite {
                address: 0xffffe,
                bytes: 2,
                value: 0x2211
            },
            Transfer::MemoryRead {
                address: 0xffffe,
                bytes: 2
            },
        ]
    );
    assert_eq!(
        low.borrow_mut().take_transfers(),
        [
            Transfer::MemoryWrite {
                address: 0,
                bytes: 2,
                value: 0x4433
            },
            Transfer::MemoryRead {
                address: 0,
                bytes: 2
            },
        ]
    );
}
