use super::{cpu, Irq};
use relocade_machine::{DispatchEntry, Hook};
use relocade_x86::{Compiler, ExecutionExit, ExecutionProfile, HostExit};
use std::{cell::RefCell, rc::Rc};

async fn queued_irq() {
    for compiled in [false, true] {
        for (code, return_ip, retired) in [
            (&[0xfb, 0x40][..], 0x102, 4), // STI; INC AX (protected successor).
            (&[0x8e, 0xd0, 0x9d][..], 0x103, 4), // MOV SS, AX; POPF (stepped IF restore).
            (&[0x9d][..], 0x101, 3),       // POPF enables IF.
            (&[0x66, 0x9d][..], 0x102, 3), // POPFD enables IF.
            (&[0xcf][..], 0x200, 3),       // IRET enables IF.
            (&[0x66, 0xcf][..], 0x200, 3), // IRETD enables IF.
        ] {
            let irq = Rc::new(RefCell::new(Irq::default()));
            let mut runner = cpu(code, 2, &irq).await;
            let memory = runner.memory();
            let width = if code[0] == 0x66 { 4 } else { 2 };
            let flags_address = if code.last() == Some(&0xcf) {
                memory.view().write_u32(0x8000, 0x200).unwrap();
                memory.view().write_u32(0x8000 + width, 0).unwrap();
                0x8000 + 2 * width
            } else {
                0x8000
            };
            memory.view().write_u32(flags_address, 0x202).unwrap();
            if compiled {
                let module = Compiler::new(ExecutionProfile::Real16)
                    .compile_block(0x100, code, 1)
                    .unwrap();
                runner
                    .install_module(
                        &module.bytes,
                        &[DispatchEntry {
                            address: 0x100,
                            export: &module.entry,
                        }],
                    )
                    .await
                    .unwrap();
            }
            runner
                .install([Hook::host(return_ip, {
                    let exit = runner.exit_handle();
                    async move || exit.request()
                })])
                .await
                .unwrap();
            irq.borrow_mut().raise();
            // The original notification was serviced while IF was clear. No
            // further wake or periodic budget may be needed to deliver this IRQ.
            runner.interrupt_handle().request();
            assert!(runner.interrupt_handle().take());
            assert_eq!(
                runner.run(u64::MAX).await,
                ExecutionExit::Host(HostExit::Return)
            );
            assert_eq!(
                irq.borrow().acknowledged,
                1,
                "compiled={compiled}, {code:x?}"
            );
            assert_eq!(memory.view().read_u16(0x500).unwrap(), 1);
            assert_eq!(runner.cpu_state().instruction_count(), retired);
            assert_eq!(runner.cpu_state().eip(), return_ip);
        }
    }
}

test_backends!(
    queued_irq_is_delivered_when_guest_enables_if_without_a_new_wake,
    queued_irq
);
