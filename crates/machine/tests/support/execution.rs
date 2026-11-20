#![allow(dead_code)]

use relocade_machine::{GuestTimeDriver, Sleep, Timestamp};
use relocade_x86::ExecutionExit;
use std::{
    future::Future,
    pin::Pin,
    task::{Context, Waker},
};

/// Registers a device wait on the guest clock. Dropping the sleep cancels it.
pub fn guest_sleep(driver: &GuestTimeDriver, tick: u64) -> Sleep {
    let mut sleep = driver.clock().sleep_until(Timestamp::from_ticks(tick));
    assert!(Pin::new(&mut sleep)
        .poll(&mut Context::from_waker(Waker::noop()))
        .is_pending());
    sleep
}

/// Bounded instruction tests use the interpreter's explicit step return.
pub fn steps(cpu: &mut relocade_machine::Cpu, count: usize) {
    for _ in 0..count {
        let exit = cpu.step();
        assert!(
            matches!(
                exit,
                ExecutionExit::StepComplete | ExecutionExit::InterruptsEnabled
            ),
            "{exit:?}"
        );
    }
}

/// Stops at the next completed guest block while retaining service/IRQ frames.
/// These tests intentionally exercise dispatch, which raw instruction stepping bypasses.
pub async fn checkpoint(cpu: &mut relocade_machine::Cpu) -> relocade_x86::ExecutionExit {
    struct Request(relocade_machine::ExecutionInterrupt);
    impl Drop for Request {
        fn drop(&mut self) {
            self.0.take();
        }
    }
    let request = Request(cpu.interrupt_handle());
    request.0.request();
    cpu.run().await
}

/// Runs dispatch to a guest boundary named by the fixture, preserving parked calls.
pub async fn run_until(cpu: &mut relocade_machine::Cpu, eip: u32) -> relocade_x86::ExecutionExit {
    // Inspect every completed block so the fixture can recognize its target EIP.
    for _ in 0..1024 {
        let exit = checkpoint(cpu).await;
        if exit != relocade_x86::ExecutionExit::Interrupted || cpu.cpu_state().eip() == eip {
            return exit;
        }
    }
    panic!(
        "guest did not reach EIP {eip:#x}; stopped at {:#x}",
        cpu.cpu_state().eip()
    );
}
