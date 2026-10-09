use super::*;

pub(super) fn register(linker: &mut Linker<ExecutionEvents>) {
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
