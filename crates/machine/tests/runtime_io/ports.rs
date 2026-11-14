//! Host input normalization and generated guest port I/O share one router.
use super::*;

fn port_bus(device: &Rc<RefCell<RecordingDevice>>) -> IoBus {
    IoBus::new(device.clone(), |io| {
        RecordingDevice::bind(io, device, 0xffff);
    })
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn host_port_access_normalizes_read_and_write_carriers() {
    let device = RecordingDevice::new(0);
    let io = port_bus(&device);
    for (bytes, value) in [(1, 0xff), (2, 0xffff), (4, u32::MAX)] {
        device.borrow_mut().port = u32::MAX;
        assert_eq!(io.read_port(0xffff, bytes), value);
        assert_eq!(io.read_port(0xfffe, bytes), value);
        io.write_port(0xffff, bytes, u32::MAX);
        assert_eq!(
            device.borrow_mut().take_transfers(),
            [
                Transfer::PortRead {
                    port: 0xffff,
                    bytes
                },
                Transfer::PortWrite {
                    port: 0xffff,
                    bytes,
                    value
                },
            ]
        );
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn invalid_host_port_widths_fail_before_device_effects() {
    use std::panic::{catch_unwind, AssertUnwindSafe};
    let device = RecordingDevice::new(0);
    let io = port_bus(&device);
    for bytes in [0, 3, 8, u8::MAX] {
        assert!(catch_unwind(AssertUnwindSafe(|| io.read_port(0xffff, bytes))).is_err());
        assert!(catch_unwind(AssertUnwindSafe(|| io.write_port(0xffff, bytes, 0))).is_err());
    }
    assert!(device.borrow_mut().take_transfers().is_empty());
}

async fn guest_carriers() {
    for (compiled, late_connection) in [(false, false), (true, false), (false, true), (true, true)]
    {
        let device = RecordingDevice::new(0);
        device.borrow_mut().port = 0x7654_3210;
        let compiler = Compiler::new(ExecutionProfile::Real16);
        let config = support::configuration(
            compiler.target(),
            CpuState {
                eip: 0x100,
                ..CpuState::real_mode()
            },
            0x1000,
            PhysicalMemoryMap::new([(0..=0xfff, PhysicalMapping::Ram { backing_offset: 0 })])
                .unwrap(),
        )
        .unwrap();
        let memory = Memory::new(config.machine).unwrap();
        let code = [0xec, 0xef]; // IN AL,DX; OUT DX,AX.
        memory.view().write(0x100, &code).unwrap();
        let io = if late_connection {
            IoBus::default()
        } else {
            port_bus(&device)
        };
        let mut cpu = Cpu::new(memory, config.cpu, io.clone()).await.unwrap();
        cpu.cpu_state().set_dx(0xffff);
        if compiled {
            let snapshots = [0, 1].map(|offset| BlockSnapshot {
                start_eip: 0x100 + offset as u32,
                bytes: &code[offset..],
                instruction_limit: 1,
            });
            let module = compiler.compile_blocks(&snapshots).unwrap();
            let entries: Vec<_> = module
                .entries
                .iter()
                .map(|entry| DispatchEntry {
                    key: entry.start_eip,
                    export: &entry.export,
                })
                .collect();
            cpu.install_module(&module.bytes, &entries).await.unwrap();
        }
        if late_connection {
            // Run already-linked modules with the unconnected defaults in both directions.
            assert_eq!(
                support::run_until(&mut cpu, 0x102).await,
                ExecutionExit::Interrupted
            );
            assert_eq!(cpu.cpu_state().al(), 0xff);
            assert!(device.borrow().transfers.is_empty());
            io.connect(device.clone(), |io| {
                RecordingDevice::bind(io, &device, 0xffff);
            });
            assert_eq!(io.read_port(0xffff, 4), 0x7654_3210);
            device.borrow_mut().take_transfers();
        }
        cpu.cpu_state().set_eip(0x100);
        cpu.cpu_state().set_eax(0x89ab_cdef);
        assert_eq!(
            support::run_until(&mut cpu, 0x102).await,
            ExecutionExit::Interrupted
        );
        assert_eq!(cpu.cpu_state().eax(), 0x89ab_cd10);
        assert_eq!(
            device.borrow_mut().take_transfers(),
            [
                Transfer::PortRead {
                    port: 0xffff,
                    bytes: 1
                },
                Transfer::PortWrite {
                    port: 0xffff,
                    bytes: 2,
                    value: 0xcd10
                },
            ]
        );
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn guest_port_callers_truncate_reads_and_clear_unused_write_bits() {
    pollster::block_on(guest_carriers());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn guest_port_callers_truncate_reads_and_clear_unused_write_bits() {
    guest_carriers().await;
}
