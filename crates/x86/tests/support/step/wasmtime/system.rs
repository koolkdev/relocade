//! Host system-register replies and observations for Wasmtime fixtures.

use super::{Caller, Event, ExecutionEvents, Linker};

pub(super) fn register(linker: &mut Linker<ExecutionEvents>) {
    linker
        .func_wrap(
            "wasm86",
            "queryDescriptorTable",
            |mut caller: Caller<'_, ExecutionEvents>, table: i32| {
                let state = caller.data_mut();
                let reply = state
                    .descriptor_table_queries
                    .next()
                    .expect("unexpected descriptor-table query");
                assert_eq!(table as u32, reply.table);
                state.events.push(Event::QueryDescriptorTable {
                    table: table as u32,
                });
                (reply.base as i32, i32::from(reply.limit))
            },
        )
        .unwrap();
}
