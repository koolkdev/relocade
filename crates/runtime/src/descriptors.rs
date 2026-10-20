//! Descriptor callbacks borrow only table registers and dedicated table bytes.

use crate::{storage::Storage, wasm::Import, MachineMemory, RuntimeBackend};
use relocade_machine::Descriptors;
use relocade_x86::{abi, Exception, Segment};

pub(crate) fn imports<B: RuntimeBackend>(
    backend: &mut B,
    memory: &MachineMemory<B::Memory>,
) -> Vec<Import<'static, B>> {
    let resolve_memory = memory.clone();
    let query_memory = memory.clone();
    let access_memory = memory.clone();
    vec![
        backend.bind_host(
            abi::RESOLVE_SEGMENT,
            Box::new(move |mut caller, destination, selector| {
                let mut storage = Storage {
                    access: &mut caller,
                    memory: resolve_memory.clone(),
                };
                let descriptors = Descriptors::new(&mut storage);
                let destination = Segment::ALL[destination as usize];
                match descriptors.resolve_user_segment(destination, selector as u16) {
                    Ok(resolved) => {
                        let cache = resolved.cache;
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
                            resolved.accessed_byte.unwrap_or(u32::MAX) as i32,
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
                            -1,
                        )
                    }
                }
            }),
        ),
        backend.bind_host(
            abi::QUERY_SEGMENT_DESCRIPTOR,
            Box::new(move |mut caller, selector| {
                let mut storage = Storage {
                    access: &mut caller,
                    memory: query_memory.clone(),
                };
                let info =
                    Descriptors::new(&mut storage).query_user_segment_descriptor(selector as u16);
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
            abi::MARK_SEGMENT_ACCESSED,
            Box::new(move |mut caller, address| {
                let mut storage = Storage {
                    access: &mut caller,
                    memory: access_memory.clone(),
                };
                Descriptors::new(&mut storage).mark_accessed(address as u32);
            }),
        ),
    ]
}
