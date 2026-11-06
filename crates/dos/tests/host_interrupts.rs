mod common;

use relocade_dos::{
    Dos, InterruptCall, InterruptError, InterruptOutcome, InterruptRegisters, ProcessStatus,
};
use relocade_machine::{ExecutionWorker, FarPointer};
use relocade_x86::{ExecutionExit, HostExit, Segment};

fn request(vector: u8, ax: u16, buffer: &[u8]) -> InterruptCall<'_> {
    InterruptCall {
        vector,
        registers: InterruptRegisters {
            ax,
            ..Default::default()
        },
        buffer,
    }
}

fn returned(outcome: InterruptOutcome) -> relocade_dos::InterruptResult {
    match outcome {
        InterruptOutcome::Returned(result) => result,
        InterruptOutcome::Exited(status) => panic!("unexpected process exit: {status:?}"),
    }
}

#[test]
fn calls_follow_guest_vectors_and_release_hooks_before_process_memory_is_reused() {
    let running = ExecutionWorker::run_until(std::future::pending::<()>(), async |runner| {
        let mut dos = Dos::new(runner).await.unwrap();
        dos.launch_host(Default::default()).unwrap();
        let process = dos.process().unwrap();
        let buffer = dos.interrupt_buffer().unwrap();
        let state = dos.cpu().cpu_state();
        let initial_sp = state.sp();
        let memory = dos.cpu().memory();
        let mut original = [0; 4];
        memory.view().read(0x21 * 4, &mut original).unwrap();
        let mut handler = vec![0x50, 0xb8, 0x34, 0x12, 0xa3]; // PUSH AX; MOV AX,1234; MOV [buffer],AX.
        handler.extend(buffer.offset.to_le_bytes());
        handler.extend([0x58, 0xea]); // POP AX; JMP FAR original INT21.
        handler.extend(original);
        memory.view().write(0x700, &handler).unwrap();
        memory.view().write_u32(0x21 * 4, 0x700).unwrap();
        let result = returned(
            dos.call_interrupt(
                runner,
                InterruptCall {
                    vector: 0x21,
                    registers: InterruptRegisters {
                        ax: 0x3000,
                        ds: buffer.segment,
                        ..Default::default()
                    },
                    buffer: &[0; 4],
                },
            )
            .await
            .unwrap(),
        );
        assert_eq!(result.registers.ax, 5);
        assert_eq!(result.buffer, [0x34, 0x12, 0, 0]);
        assert_eq!(state.segment(Segment::Cs).selector, process.psp_segment());
        assert_eq!(state.eip(), 0x102);
        assert_eq!(state.sp(), initial_sp);
        assert!(!dos.cpu().has_pending_host_calls());
        let idle = state.snapshot();
        assert_eq!(
            dos.run(runner).await,
            Err(relocade_dos::RunError::HostProcess)
        );
        assert_eq!(state.snapshot(), idle);

        memory
            .view()
            .write(0x750, &[0xb8, 0xef, 0xbe, 0xcf])
            .unwrap(); // MOV AX,BEEF; IRET.
        memory.view().write_u32(0x60 * 4, 0x750).unwrap();
        let result = returned(
            dos.call_interrupt(runner, request(0x60, 0, &[]))
                .await
                .unwrap(),
        );
        assert_eq!(
            result.registers.ax, 0xbeef,
            "the next call can select another vector"
        );
        memory.view().write(0x21 * 4, &original).unwrap();
        assert!(matches!(
            dos.call_interrupt(runner, request(0x21, 0x4c07, &[]))
                .await
                .unwrap(),
            InterruptOutcome::Exited(Ok(7))
        ));
        assert_eq!(process.status(), ProcessStatus::Exited(Ok(7)));
        // Enter precisely the old completion address before terminating normally.
        dos.launch_com(&[0xeb, 0, 0xb8, 9, 0x4c, 0xcd, 0x21], Default::default())
            .unwrap();
        assert_eq!(dos.run(runner).await, Ok(9));
    });
    assert_eq!(pollster::block_on(running).unwrap(), Some(()));
}

#[test]
fn cancelled_and_completed_calls_keep_their_buffer_until_explicitly_resumed() {
    use futures_channel::oneshot;
    use futures_util::future::{select, Either};
    use relocade_machine::real_mode_interrupt::Interrupts;
    use std::{cell::RefCell, pin::pin};

    let running = ExecutionWorker::run_until(std::future::pending::<()>(), async |runner| {
        let mut dos = Dos::new(runner).await.unwrap();
        dos.launch_host(Default::default()).unwrap();
        let memory = dos.cpu().memory();
        let state = dos.cpu().cpu_state();
        let buffer = dos.interrupt_buffer().unwrap();
        let (entered, waiting) = oneshot::channel();
        let (release, gate) = oneshot::channel();
        let entered = RefCell::new(Some(entered));
        let gate = RefCell::new(Some(gate));
        let mut interrupts = Interrupts::new(dos.cpu());
        interrupts.register_interrupt(
            0x60,
            FarPointer {
                segment: 0,
                offset: 0x700,
            },
            async move |call| {
                let gate = gate.borrow_mut().take().unwrap();
                entered.borrow_mut().take().unwrap().send(()).unwrap();
                gate.await.unwrap();
                call.cpu.set_ax(0x1234);
            },
        );
        interrupts.install().await.unwrap();
        {
            let call = pin!(dos.call_interrupt(runner, request(0x60, 0, b"owned")));
            assert!(matches!(
                select(call, waiting).await,
                Either::Right((Ok(()), _))
            ));
        }
        let pending = state.snapshot();
        assert!(matches!(
            dos.call_interrupt(runner, request(0x21, 0, b"wrong")).await,
            Err(InterruptError::Busy)
        ));
        assert_eq!(state.snapshot(), pending);
        let mut bytes = [0; 5];
        memory.view().read(buffer.address(), &mut bytes).unwrap();
        assert_eq!(&bytes, b"owned");
        release.send(()).unwrap();
        // A caller can finish driving the CPU before recovering the Rust result.
        assert_eq!(
            runner.run_cpu(dos.cpu()).await,
            ExecutionExit::Host(HostExit::Return)
        );
        assert!(!dos.cpu().has_pending_host_calls());
        assert!(matches!(
            dos.call_interrupt(runner, request(0x21, 0, b"wrong")).await,
            Err(InterruptError::Busy)
        ));
        let result = returned(dos.resume_interrupt(runner).await.unwrap());
        assert_eq!(result.registers.ax, 0x1234);
        assert_eq!(result.buffer, b"owned");
        assert!(matches!(
            dos.resume_interrupt(runner).await,
            Err(InterruptError::NoPendingCall)
        ));

        // This call parks at Runner's initial yield, before a host service exists.
        common::assert_pending(dos.call_interrupt(runner, request(0x21, 0x3000, b"queued")));
        assert!(!dos.cpu().has_pending_host_calls());
        assert!(matches!(
            dos.call_interrupt(runner, request(0x21, 0, b"wrong")).await,
            Err(InterruptError::Busy)
        ));
        let result = returned(dos.resume_interrupt(runner).await.unwrap());
        assert_eq!(result.registers.ax, 5);
        assert_eq!(result.buffer, b"queued");

        let (entered, waiting) = oneshot::channel();
        let entered = RefCell::new(Some(entered));
        let mut interrupts = Interrupts::new(dos.cpu());
        interrupts.register_interrupt(
            0x61,
            FarPointer {
                segment: 0,
                offset: 0x800,
            },
            async move |_| {
                entered.borrow_mut().take().unwrap().send(()).unwrap();
                std::future::pending::<()>().await;
            },
        );
        interrupts.install().await.unwrap();
        {
            let call = pin!(dos.call_interrupt(runner, request(0x61, 0, &[])));
            assert!(matches!(
                select(call, waiting).await,
                Either::Right((Ok(()), _))
            ));
        }
        assert!(dos.cpu().has_pending_host_calls());
        dos.cpu().exit_handle().request();
        assert_eq!(
            runner.run_cpu(dos.cpu()).await,
            ExecutionExit::Host(HostExit::Return)
        );
        assert!(!dos.cpu().has_pending_host_calls());
        assert!(matches!(
            dos.resume_interrupt(runner).await,
            Err(InterruptError::Aborted)
        ));

        dos.cpu().exit_handle().request();
        assert!(matches!(
            dos.call_interrupt(runner, request(0x21, 0x3000, &[])).await,
            Err(InterruptError::Aborted)
        ));
        assert!(matches!(
            dos.resume_interrupt(runner).await,
            Err(InterruptError::NoPendingCall)
        ));
        assert_eq!(
            returned(
                dos.call_interrupt(runner, request(0x21, 0x3000, &[]))
                    .await
                    .unwrap()
            )
            .registers
            .ax,
            5
        );
    });
    assert_eq!(pollster::block_on(running).unwrap(), Some(()));
}
