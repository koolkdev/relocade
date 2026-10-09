use super::*;

pub(super) fn register(linker: &mut Linker<ExecutionEvents>) {
    linker
        .func_wrap(
            "wasm86",
            "readTimestampCounter",
            |mut caller: Caller<'_, ExecutionEvents>| {
                let state = caller.data_mut();
                state.events.push(Event::TimestampRead);
                let Some(Argument::I64(counter)) = state.timestamp_reads.next() else {
                    panic!("expected a timestamp reply with an i64 carrier");
                };
                counter
            },
        )
        .unwrap();
    linker
        .func_wrap(
            "wasm86",
            "cpuid",
            |mut caller: Caller<'_, ExecutionEvents>, leaf: u32, subleaf: u32| {
                let state = caller.data_mut();
                state.events.push(Event::Cpuid { leaf, subleaf });
                let [eax, ebx, ecx, edx] =
                    state.cpuid_results.next().expect("unexpected CPUID query");
                (eax, ebx, ecx, edx)
            },
        )
        .unwrap();
}
