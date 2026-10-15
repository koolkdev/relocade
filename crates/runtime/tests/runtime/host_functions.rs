use std::{cell::Cell, rc::Rc};

use super::{machine, TestMachine};
use wasm86_machine::{HostAction, HostContext, HostFunctions, HostReturn};

async fn install(
    machine: &mut TestMachine,
    address: u32,
    return_: HostReturn,
    callback: impl FnMut(&mut HostContext<'_, ()>) -> HostAction + 'static,
) {
    let mut functions = HostFunctions::new();
    functions.insert(address, return_, callback);
    machine.install_host_functions(functions).await.unwrap();
}

async fn count_calls(machine: &mut TestMachine, address: u32, action: HostAction) -> Rc<Cell<u32>> {
    let count = Rc::new(Cell::new(0));
    let calls = count.clone();
    install(machine, address, HostReturn::Interrupt, move |_| {
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

#[path = "host_functions/continuation.rs"]
mod continuation;
#[path = "host_functions/interrupts.rs"]
mod interrupts;
#[path = "host_functions/lookup.rs"]
mod lookup;
