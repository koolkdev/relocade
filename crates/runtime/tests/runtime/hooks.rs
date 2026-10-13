use std::{cell::Cell, rc::Rc};

use super::{machine, TestMachine};
use wasm86_runtime::{HookReturn, Hooks, HostAction, HostContext};

async fn install(
    machine: &mut TestMachine,
    address: u32,
    return_: HookReturn,
    callback: impl FnMut(&mut HostContext<'_, ()>) -> HostAction + 'static,
) {
    let mut hooks = Hooks::new();
    hooks.insert(address, return_, callback);
    machine.install_hooks(hooks).await.unwrap();
}

async fn count_calls(machine: &mut TestMachine, address: u32, action: HostAction) -> Rc<Cell<u32>> {
    let count = Rc::new(Cell::new(0));
    let calls = count.clone();
    install(machine, address, HookReturn::Interrupt, move |_| {
        calls.set(calls.get() + 1);
        action
    })
    .await;
    count
}

fn return_frame(machine: &mut TestMachine, address: u16) {
    machine
        .write_memory(
            machine.cpu().registers.esp,
            &[address as u8, (address >> 8) as u8, 0, 0, 2, 0],
        )
        .unwrap();
}

#[path = "hooks/continuation.rs"]
mod continuation;
#[path = "hooks/interrupts.rs"]
mod interrupts;
#[path = "hooks/lookup.rs"]
mod lookup;
