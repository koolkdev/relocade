use super::*;

pub(super) fn register(linker: &mut Linker<ExecutionEvents>, guest: Memory, table: Option<Memory>) {
    linker
        .func_wrap(
            "wasm86",
            "readPort",
            move |mut caller: Caller<'_, ExecutionEvents>, port: u32, bytes: u32| {
                let events = caller.data_mut();
                events.events.push(Event::PortRead { port, bytes });
                let value = events.port_reads.next().expect("unexpected port read");
                let update = events.port_updates.next();
                physical::apply_update(&mut caller, guest, table, update);
                value
            },
        )
        .unwrap();
    linker
        .func_wrap(
            "wasm86",
            "writePort",
            move |mut caller: Caller<'_, ExecutionEvents>, port: u32, bytes: u32, value: u32| {
                caller
                    .data_mut()
                    .events
                    .push(Event::PortWrite { port, bytes, value });
                let update = caller.data_mut().port_updates.next();
                physical::apply_update(&mut caller, guest, table, update);
            },
        )
        .unwrap();
}
