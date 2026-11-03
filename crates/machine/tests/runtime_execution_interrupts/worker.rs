use super::*;
use futures_channel::oneshot;
use relocade_machine::{
    worker::{Thread, ThreadExt},
    Device, DeviceConnection,
};
use std::{cell::RefCell, rc::Rc};

struct Started(Option<oneshot::Sender<()>>);

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
        // A finite fallback fails the assertion if the guest never observes the
        // other worker. No host yield or service poll delivers this notification.
        let exit = cpu.run(10_000_000).await;
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
