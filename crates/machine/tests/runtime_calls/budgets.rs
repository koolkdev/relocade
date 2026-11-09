use super::{cpu, pending, Input, Irq, Operation};
use relocade_machine::real_mode_interrupt;
use relocade_x86::{ExecutionExit, HostExit};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

async fn suspended_service() {
    let irq = Rc::new(RefCell::new(Irq::default()));
    let mut runner = cpu(&irq, 0x300).await;
    let state = runner.cpu_state();
    let input = Rc::new(Input::default());
    let drops = Rc::new(Cell::new(0));
    runner
        .install(real_mode_interrupt::hook(0x200, state.clone(), {
            let (state, input, drops, exit) = (
                state.clone(),
                input.clone(),
                drops.clone(),
                runner.exit_handle(),
            );
            async move || {
                let _operation = Operation(drops.clone());
                state.set_interrupts_enabled(true);
                let first = input.read().await;
                let saved = [first];
                let local = &saved[0];
                let second = input.read().await;
                state.set_ax(*local + second);
                exit.request();
            }
        }))
        .await
        .unwrap();
    input.send(10);
    let target = runner.instruction_target();
    target.set(Some(10));
    pending(runner.run());
    irq.borrow_mut().raise();
    target.set(Some(state.instruction_count() + 1));
    assert_eq!(runner.run().await, ExecutionExit::BudgetExhausted);
    assert_eq!(state.eip(), 0x306);
    assert_eq!(state.instruction_count(), 3); // INT 21h, then ISR INC and JMP.
    assert_eq!(drops.get(), 0);
    target.set(None);
    input.send(7);
    assert_eq!(input.consumed.get(), 1); // The service stays parked until guest IRET.
    assert_eq!(runner.run().await, ExecutionExit::Host(HostExit::Return));
    assert_eq!(state.ax(), 17);
    assert_eq!(state.instruction_count(), 4);
    assert_eq!(drops.get(), 1);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn budget_exhaustion_preserves_a_suspended_service_and_its_locals() {
    pollster::block_on(suspended_service());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn budget_exhaustion_preserves_a_suspended_service_and_its_locals() {
    suspended_service().await;
}
