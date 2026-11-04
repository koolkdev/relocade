use super::*;
use futures_channel::oneshot;
use relocade_machine::{
    worker::{Thread, ThreadExt},
    CpuConfig, CpuStorage, Device, DeviceConnection, MachineConfig,
};
use std::{cell::RefCell, rc::Rc};

struct Started(Option<oneshot::Sender<()>>);

async fn prepared_storage() {
    let storage = CpuStorage::new().unwrap();
    let interrupt = storage.interrupt_handle();
    interrupt.request();
    let worker = Thread::new();
    let task = worker.run_resource(storage, |storage| async move {
        let map =
            PhysicalMemoryMap::new([(0..=0xffff, PhysicalMapping::Ram { backing_offset: 0 })])
                .unwrap();
        let memory = Memory::new(MachineConfig::new(0x10000, map).unwrap()).unwrap();
        memory.view().write(0x100, &[0x40, 0xeb, 0xfd]).unwrap();
        let initial = CpuState {
            eip: 0x100,
            segments: Segments::real_mode(),
            ..CpuState::default()
        };
        let config =
            CpuConfig::new(Compiler::new(ExecutionProfile::Real16).target(), initial).unwrap();
        let mut cpu = storage
            .into_cpu(memory, config, IoBus::default())
            .await
            .unwrap();
        assert_eq!(cpu.cpu_state().snapshot(), initial);
        assert_eq!(cpu.run().await, ExecutionExit::Interrupted);
        assert_eq!(cpu.cpu_state().ax(), 1);
        // The host's original handle still addresses this CPU's live signal.
        cpu.cpu_state().instruction_count()
    });
    assert_eq!(task.await.unwrap(), 2);
    assert!(interrupt.take());
    assert!(!interrupt.take());
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn prepared_cpu_storage_preserves_notifications_across_worker_construction() {
    pollster::block_on(prepared_storage());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn prepared_cpu_storage_preserves_notifications_across_worker_construction() {
    prepared_storage().await;
}

impl Device for Started {
    fn write_port(&mut self, _port: u16, _bytes: u8, _value: u32) {
        self.0.take().unwrap().send(()).unwrap();
    }
}

async fn request_during_execution() {
    for compiled in [false, true] {
        let (entered, running) = oneshot::channel();
        let io = IoBus::new([DeviceConnection {
            device: Rc::new(RefCell::new(Started(Some(entered)))),
            ports: vec![0x80..=0x80],
            mmio: vec![],
        }]);
        // OUT signals entry; INC AX / JMP then stay inside guest execution.
        let code = [0xe6, 0x80, 0x40, 0xeb, 0xfd];
        let mut cpu = cpu(&code, compiled, io).await;
        if compiled {
            let module = Compiler::new(ExecutionProfile::Real16)
                .compile_block(0x102, &code[2..], 2)
                .unwrap();
            cpu.install_module(
                &module.bytes,
                &[DispatchEntry {
                    address: 0x102,
                    export: &module.entry,
                }],
            )
            .await
            .unwrap();
        }
        let interrupt = cpu.interrupt_handle();
        let notifier = Thread::new();
        let (ready, listening) = oneshot::channel();
        let notified = notifier.run_resource(interrupt.clone(), move |interrupt| async move {
            ready.send(()).unwrap();
            running.await.unwrap();
            interrupt.request();
        });
        listening.await.unwrap();
        // Only the other worker's request can leave this guest loop; no host
        // yield, instruction budget or service poll delivers the notification.
        let exit = cpu.run().await;
        notified.await.unwrap();
        drop(notifier);
        assert_eq!(exit, ExecutionExit::Interrupted);
        assert!(!cpu.cpu_state().interrupts_enabled());
        assert!(interrupt.take());
        assert!(!interrupt.take());
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn another_worker_requests_execution_interrupt_during_guest_execution() {
    pollster::block_on(request_during_execution());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn another_worker_requests_execution_interrupt_during_guest_execution() {
    request_during_execution().await;
}
