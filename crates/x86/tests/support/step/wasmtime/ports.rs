use super::*;

pub(super) fn register(linker: &mut Linker<ExecutionEvents>) {
    linker
        .func_wrap(
            "wasm86",
            "readPort",
            |mut caller: Caller<'_, ExecutionEvents>, port: u32, bytes: u32| {
                let events = caller.data_mut();
                events.events.push(Event::PortRead { port, bytes });
                events.port_reads.next().expect("unexpected port read")
            },
        )
        .unwrap();
    linker
        .func_wrap(
            "wasm86",
            "writePort",
            |mut caller: Caller<'_, ExecutionEvents>, port: u32, bytes: u32, value: u32| {
                caller
                    .data_mut()
                    .events
                    .push(Event::PortWrite { port, bytes, value });
            },
        )
        .unwrap();
}
