use super::*;
use relocade_machine::{Hook, PortBinding, PortRead, PortWrite};
use relocade_x86::HostExit;

macro_rules! test_backends {
    ($name:ident, $check:ident) => {
        #[cfg(not(target_arch = "wasm32"))]
        #[test]
        fn $name() {
            pollster::block_on($check());
        }
        #[cfg(target_arch = "wasm32")]
        #[wasm_bindgen_test::wasm_bindgen_test]
        async fn $name() {
            $check().await;
        }
    };
}

#[path = "mappings/configuration.rs"]
mod configuration;
#[path = "mappings/mmio.rs"]
mod mmio;
#[path = "mappings/transfers.rs"]
mod transfers;

async fn guest(io: IoBus, code: &[u8], entries: &[usize], compiled: bool) -> Cpu {
    let compiler = Compiler::new(ExecutionProfile::Real16);
    let config = support::configuration(
        compiler.target(),
        CpuState {
            eip: 0x100,
            ..CpuState::real_mode()
        },
        0x5000,
        map(),
    )
    .unwrap();
    let memory = Memory::new(config.machine).unwrap();
    memory.view().write(0x100, code).unwrap();
    let mut cpu = Cpu::new(memory, config.cpu, io).await.unwrap();
    if compiled {
        let snapshots: Vec<_> = entries
            .iter()
            .map(|&offset| BlockSnapshot {
                start_eip: 0x100 + offset as u32,
                bytes: &code[offset..],
                instruction_limit: 16,
            })
            .collect();
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
    let exit = cpu.exit_handle();
    cpu.install([Hook::host(0x100 + code.len() as u32, async move || {
        exit.request()
    })])
    .await
    .unwrap();
    cpu
}

async fn run(cpu: &mut Cpu) {
    assert_eq!(cpu.run().await, ExecutionExit::Host(HostExit::Return));
}

async fn read(cpu: &mut Cpu, expected: u32) {
    cpu.context().cpu.set_eip(0x100);
    cpu.context().cpu.set_edx(0xabcd_ffff);
    run(cpu).await;
    assert_eq!(cpu.context().cpu.eax(), expected);
}

fn bind(io: &IoBus, port: u16) -> PortBinding {
    let mut binding = None;
    io.connect(RecordingDevice::new(0), |io| binding = Some(io.port(port)));
    binding.unwrap()
}

async fn check_shared_bindings() {
    for compiled in [false, true] {
        let device = RecordingDevice::new(0);
        device.borrow_mut().port = 0x1234_5678;
        let mut bindings = None;
        let io = IoBus::new(device.clone(), |io| {
            RecordingDevice::bind(io, &device, 0xfffe..=0xffff);
            bindings = Some((io.port(0xfffe..=0xffff), io.port(0xffff), io.port(0x3d5)));
        });
        let (range, port, start) = bindings.unwrap();
        let recorded = RecordingDevice::read_operation(&device);
        let mut cpu = guest(io.clone(), &[0x66, 0xed], &[0], compiled).await;
        read(&mut cpu, 0x1234_5678).await;
        let reusable = PortRead::new(|_, _, _| 17);
        io.with_context(|context| range.set_read(context, &reusable));
        read(&mut cpu, 17).await;
        io.with_context(|context| port.set_read(context, &PortRead::new(|_, _, _| 29)));
        read(&mut cpu, 29).await;
        assert_eq!(io.read_port(0xfffe, 4), 17);
        io.with_context(|context| port.set_read(context, &recorded));
        read(&mut cpu, 0x1234_5678).await;
        // Selecting reads does not change writes, and a wide transfer stays one call.
        device.borrow_mut().take_transfers();
        io.write_port(0xffff, 4, 0x7654_3210);
        assert_eq!(
            device.borrow_mut().take_transfers(),
            [Transfer::PortWrite {
                port: 0xffff,
                bytes: 4,
                value: 0x7654_3210,
            }]
        );
        read(&mut cpu, 0x7654_3210).await;
        io.with_context(|context| port.set_read(context, &reusable));
        drop(port);
        read(&mut cpu, 17).await;

        let started = Rc::new(std::cell::Cell::new(0));
        let captured = started.clone();
        io.with_context(|context| {
            start.set_read(
                context,
                &PortRead::new(move |_, port, bytes| {
                    assert_eq!((port, bytes), (0x3d5, 1));
                    captured.set(captured.get() + 1);
                    0
                }),
            )
        });
        cpu.install_module(START_READ, &[]).await.unwrap();
        assert_eq!(started.get(), 1);
    }
}

test_backends!(
    fixed_bindings_select_handlers_for_host_and_guest,
    check_shared_bindings
);

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn retained_bindings_and_recorded_handlers_do_not_keep_the_bus_or_device_alive() {
    let device = RecordingDevice::new(0);
    let weak = Rc::downgrade(&device);
    let recorded = RecordingDevice::read_operation(&device);
    let mut port = None;
    let io = IoBus::new(device.clone(), |io| {
        let binding = io.port(0x80);
        binding.set_read(io.context(), &recorded);
        port = Some(binding);
    });
    drop(device);
    assert!(weak.upgrade().is_some());
    drop(io);
    assert!(weak.upgrade().is_none());
    drop((port, recorded));
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn foreign_contexts_and_handlers_are_rejected_before_mutation() {
    use std::panic::{catch_unwind, AssertUnwindSafe};
    let io = IoBus::default();
    let port = bind(&io, 0x80);
    let read = PortRead::new(|_, _, _| 17);
    let write = PortWrite::new(|_, _, _, _| {});
    io.with_context(|mut context| {
        port.set_read(context.reborrow(), &read);
        port.set_write(context, &write);
    });
    let foreign = IoBus::default();
    let foreign_port = bind(&foreign, 0x80);
    assert!(
        catch_unwind(AssertUnwindSafe(|| foreign.with_context(|context| {
            port.set_read(context, &PortRead::new(|_, _, _| 29));
        })))
        .is_err()
    );
    assert_eq!(io.read_port(0x80, 1), 17);
    // Handler affinity is fixed at installation, before either bus has a CPU.
    assert!(
        catch_unwind(AssertUnwindSafe(|| foreign.with_context(|context| {
            foreign_port.set_read(context, &read);
        })))
        .is_err()
    );
    assert!(
        catch_unwind(AssertUnwindSafe(|| foreign.with_context(|context| {
            foreign_port.set_write(context, &write);
        })))
        .is_err()
    );
    assert_eq!(foreign.read_port(0x80, 1), 0xff);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn claims_are_fixed_and_overlapping_ranges_are_rejected() {
    use std::panic::{catch_unwind, AssertUnwindSafe};
    let io = IoBus::default();
    drop(bind(&io, 0x80));
    assert!(catch_unwind(AssertUnwindSafe(|| bind(&io, 0x80))).is_err());
    let device = RecordingDevice::new(0);
    io.connect(device.clone(), |io| {
        RecordingDevice::bind(io, &device, 0x70..=0x71);
        drop(io.port(0x71));
        assert!(catch_unwind(AssertUnwindSafe(|| io.port(0x71..=0x72))).is_err());
        drop(io.port(0x72));
    });
    assert_eq!(io.read_port(0x70, 1), 0);
}

// Imports portReads and starts a function equivalent to:
// (drop (call_indirect (type (func (param i32 i32) (result i32)))
//   (i32.const 0x3d5) (i32.const 1) (i32.const 0x3d5)))
const START_READ: &[u8] = &[
    0, 97, 115, 109, 1, 0, 0, 0, 1, 10, 2, 96, 2, 127, 127, 1, 127, 96, 0, 0, 2, 29, 1, 8, b'r',
    b'e', b'l', b'o', b'c', b'a', b'd', b'e', 9, b'p', b'o', b'r', b't', b'R', b'e', b'a', b'd',
    b's', 1, 112, 1, 128, 128, 4, 128, 128, 4, 3, 2, 1, 1, 8, 1, 0, 10, 16, 1, 14, 0, 65, 213, 7,
    65, 1, 65, 213, 7, 17, 0, 0, 26, 11,
];
