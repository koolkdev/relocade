use super::{
    setup, CompilerService, Duration, ExecutionExit, FarPointer, Hook, HostExit, NonZeroUsize,
};
use crate::{real_mode_interrupt::Interrupts, Cpu, ExecutionWorker, SessionConfig};
use futures_channel::{mpsc, oneshot};
use futures_util::{future::Either, FutureExt, StreamExt};
use relocade_x86::{CallCount, CodeAddress};
use std::{
    cell::Cell,
    future::Future,
    pin::pin,
    sync::Arc,
    task::{Context, Wake, Waker},
};

fn pointer(offset: u16) -> FarPointer {
    FarPointer { segment: 0, offset }
}

fn submit_hot_function(cpu: &mut Cpu, target: u32) {
    let code = &mut cpu.instance.code;
    code.profiling.hotness.record(CallCount {
        target,
        count: 4096,
    });
    code.submit_hot_functions();
}

struct CpuWake(mpsc::UnboundedSender<()>);

impl CpuWake {
    fn channel() -> (Waker, mpsc::UnboundedReceiver<()>) {
        let (signal, wakes) = mpsc::unbounded();
        (Waker::from(Arc::new(Self(signal))), wakes)
    }
}

impl Wake for CpuWake {
    fn wake(self: Arc<Self>) {
        let _ = self.0.unbounded_send(());
    }
}

async fn installation_during_nested_service() {
    let compiler = CompilerService::start_with_workers(NonZeroUsize::new(1).unwrap()).unwrap();
    let (mut machine, config) = setup();
    let cpu = machine.create_cpu(config, Some(&compiler)).unwrap();
    let result = ExecutionWorker::spawn(cpu, SessionConfig::host(), async |mut runner| {
        let cpu = runner.cpu();
        cpu.cpu_state().set_sp(0x800);
        let memory = cpu.memory();
        memory
            .view()
            .write(0x100, &[0xcd, 0x60, 0xe9, 0xfe, 0x03]) // INT 60h; JMP 0503
            .unwrap();
        memory.view().write(0x400, &[0xcd, 0x61, 0xcb]).unwrap(); // INT 61h; RETF

        // The compiler exports both the root and the return address of CALL.
        // Real16 discovery follows HLT's successor, so end that path with RET.
        memory
            .view()
            .write(0x500, &[0xe8, 0x0d, 0x00, 0x40, 0xf4, 0xc3]) // CALL 0510; INC AX; HLT; RET
            .unwrap();
        memory.view().write(0x510, &[0xc3]).unwrap(); // RET
        memory.view().write(0x520, &[0x42, 0xc3]).unwrap(); // INC DX; RET

        let (release, gate) = oneshot::channel::<()>();
        let gate = gate.shared();
        let (signal, entered) = oneshot::channel::<()>();
        let signal = Cell::new(Some(signal));
        let mut entries = Interrupts::new(cpu);
        entries.register_interrupt(0x60, pointer(0x300), async |call| {
            call.call_far(pointer(0x400), &[]).await;
        });
        entries.register_interrupt(0x61, pointer(0x320), async move |_| {
            signal.take().unwrap().send(()).unwrap();
            gate.clone().await.unwrap();
        });
        entries.install().await.unwrap();
        let exit = cpu.exit_handle();
        cpu.install([Hook::host(0x503, async move || exit.request())])
            .await
            .unwrap();

        match runner.run_cpu_until(entered).await {
            Either::Right(entered) => entered.unwrap(),
            Either::Left(exit) => panic!("guest exited before nested service: {exit:?}"),
        }
        let cpu = runner.cpu();
        assert!(cpu.pending.is_some() && !cpu.suspended.is_empty());
        submit_hot_function(cpu, 0x500);

        // Both runs register before yielding to the local reply transport.
        // Drop the first run while the request is still outstanding.
        let (previous, mut previous_wakes) = CpuWake::channel();
        assert!(pin!(cpu.run())
            .poll(&mut Context::from_waker(&previous))
            .is_pending());
        assert!(!cpu.instance.code.modules.has_entry(0x500));
        {
            let (current, mut wakes) = CpuWake::channel();
            let mut run = pin!(cpu.run());
            let mut cx = Context::from_waker(&current);
            assert!(run.as_mut().poll(&mut cx).is_pending());
            wakes.next().await.unwrap();
            assert!(matches!(
                previous_wakes.try_recv(),
                Err(mpsc::TryRecvError::Empty)
            ));
            assert!(run.as_mut().poll(&mut cx).is_pending());
        }
        assert!(cpu.pending.is_some() && !cpu.suspended.is_empty());
        assert!(cpu.instance.code.modules.has_entry(0x500));
        let module = cpu.compiled_modules_at(0x500).next().unwrap();
        assert!(module.blocks().iter().any(|block| matches!(
            block.address,
            CodeAddress::RealMode {
                segment: 0,
                eip: 0x503
            }
        )));

        submit_hot_function(cpu, 0x520);
        {
            let (current, mut wakes) = CpuWake::channel();
            let mut run = pin!(cpu.run());
            let mut cx = Context::from_waker(&current);
            assert!(run.as_mut().poll(&mut cx).is_pending());

            // The second reply is local. Patch the first CALL's displacement
            // before the parked run gets its next poll.
            wakes.next().await.unwrap();
            memory.view().write_u8(0x501, 0x0c).unwrap();
            let first = memory.installed_code_at(0x1000 + 0x501);
            assert!(matches!(first.as_slice(), [module] if module.was_modified()));
            assert!(memory.installed_code_at(0x1000 + 0x520).is_empty());
            assert!(run.as_mut().poll(&mut cx).is_pending());
        }
        assert!(cpu.pending.is_some() && !cpu.suspended.is_empty());
        assert_eq!(cpu.compiled_modules_at(0x500).count(), 0);
        assert_eq!(cpu.compiled_modules_at(0x520).count(), 1);

        // After both services return, the jump to 0503 must still reach the
        // original exit hook. Executing the guest bytes there would halt.
        release.send(()).unwrap();
        loop {
            let exit = cpu.run().await;
            if exit != ExecutionExit::Interrupted {
                assert_eq!(exit, ExecutionExit::Host(HostExit::Return));
                break;
            }
            cpu.interrupt_handle().take();
            crate::yield_now().await;
        }
        assert!(cpu.pending.is_none() && cpu.suspended.is_empty());
    })
    .run_until(futures_timer::Delay::new(Duration::from_secs(20)))
    .await;
    machine.shutdown().await.unwrap();
    compiler.shutdown().await.unwrap();
    assert_eq!(result.unwrap(), Some(()));
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn compiler_reply_wakes_the_replacement_cpu_run_while_nested_services_wait() {
    pollster::block_on(installation_during_nested_service());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn compiler_reply_wakes_the_replacement_cpu_run_while_nested_services_wait() {
    installation_during_nested_service().await;
}
