//! Descriptor callbacks borrow only table registers and dedicated table bytes.

use crate::Descriptors;
use crate::{CpuBindings, CpuView, RuntimeBackend};
use relocade_wasm_runtime::{Import, MemoryBytes, WasmMemoryAccess};
use relocade_x86::{abi, Exception, Segment};

pub(crate) fn imports<B: RuntimeBackend>(
    backend: &mut B,
    bindings: &CpuBindings<B::Memory>,
) -> Vec<Import<'static, B>> {
    let resolve_bindings = bindings.clone();
    let profile = bindings.config.execution_target().profile;
    let query_bindings = bindings.clone();
    let completion_bindings = bindings.clone();
    vec![
        backend.bind_host(
            abi::RESOLVE_SEGMENT,
            Box::new(move |mut caller, destination, selector| {
                let memory = caller
                    .shared_memory(&resolve_bindings.descriptors)
                    .expect("shared descriptors");
                let cpu = CpuView::new(
                    &resolve_bindings.config,
                    caller.memory_bytes(&resolve_bindings.context),
                );
                let descriptors = Descriptors::new(MemoryBytes::shared(&memory), cpu);
                let destination = Segment::ALL[destination as usize];
                match descriptors.resolve_user_segment(destination, selector as u16) {
                    Ok(resolved) => {
                        let cache = resolved.cache();
                        assert!(
                            profile.is_segment_compatible(destination, &cache),
                            "segment {destination:?} violates execution profile {profile:?}"
                        );
                        (
                            0,
                            0,
                            cache.base as i32,
                            cache.limit as i32,
                            i32::from(cache.selector),
                            i32::from(cache.usable),
                            i32::from(cache.readable),
                            i32::from(cache.writable),
                            i32::from(cache.executable),
                            i32::from(cache.expand_down),
                            i32::from(cache.big),
                            i32::from(resolved.needs_completion()),
                        )
                    }
                    Err(exception) => {
                        let code = match exception {
                            Exception::GeneralProtection { error_code }
                            | Exception::StackFault { error_code }
                            | Exception::SegmentNotPresent { error_code } => error_code,
                            _ => unreachable!(
                                "dedicated descriptor memory produces only segment faults"
                            ),
                        };
                        (
                            exception.vector() as i32,
                            code as i32,
                            0,
                            0,
                            0,
                            0,
                            0,
                            0,
                            0,
                            0,
                            0,
                            0,
                        )
                    }
                }
            }),
        ),
        backend.bind_host(
            abi::QUERY_SEGMENT_DESCRIPTOR,
            Box::new(move |mut caller, selector| {
                let memory = caller
                    .shared_memory(&query_bindings.descriptors)
                    .expect("shared descriptors");
                let cpu = CpuView::new(
                    &query_bindings.config,
                    caller.memory_bytes(&query_bindings.context),
                );
                let descriptors = Descriptors::new(MemoryBytes::shared(&memory), cpu);
                let info = descriptors.query_user_segment_descriptor(selector as u16);
                (
                    i32::from(info.lar_valid),
                    i32::from(info.lsl_valid),
                    i32::from(info.readable),
                    i32::from(info.writable),
                    info.access_rights as i32,
                    info.limit as i32,
                )
            }),
        ),
        backend.bind_host(
            abi::COMPLETE_SEGMENT_LOAD,
            Box::new(move |mut caller, selector| {
                let memory = caller
                    .shared_memory(&completion_bindings.descriptors)
                    .expect("shared descriptors");
                let cpu = CpuView::new(
                    &completion_bindings.config,
                    caller.memory_bytes(&completion_bindings.context),
                );
                let mut descriptors = Descriptors::new(MemoryBytes::shared(&memory), cpu);
                descriptors
                    .mark_accessed(selector as u16)
                    .expect("segment completion uses unchanged CPU table registers");
            }),
        ),
    ]
}
