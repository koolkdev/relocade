//! The interpreter and compiled modules resolve selectors against this CPU's host tables.

use relocade_wasm_runtime::{FunctionRef, Import, WasmBackend, WasmRuntime};
use relocade_x86::{abi, DescriptorTables, Exception, Segment, SegmentProfile, SegmentRegister};
use std::sync::{Arc, Mutex};

pub(super) fn imports(
    wasm: &mut WasmRuntime,
    tables: Arc<Mutex<DescriptorTables>>,
) -> [Import<'static, WasmRuntime>; 3] {
    let mut context = wasm.context();
    let resolve_tables = tables.clone();
    let resolve = FunctionRef::new(&mut context, move |_, (segment, selector): (i32, i32)| {
        let destination = Segment::ALL[segment as usize];
        let answer = resolve_tables
            .lock()
            .unwrap()
            .resolve_user_segment(destination, selector as u16);
        let (status, error, cache) = match answer {
            Ok(cache) if cache.is_compatible_with(SegmentProfile::Flat32, destination) => {
                (0, 0, cache)
            }
            Ok(_) => (
                abi::UNSUPPORTED_SEGMENT_LOAD as i32,
                0,
                SegmentRegister::default(),
            ),
            Err(fault) => {
                let error_code = match fault {
                    Exception::GeneralProtection { error_code }
                    | Exception::StackFault { error_code }
                    | Exception::SegmentNotPresent { error_code } => error_code,
                    _ => unreachable!("user segment resolution reports only segment faults"),
                };
                (
                    fault.vector() as i32,
                    error_code as i32,
                    SegmentRegister::default(),
                )
            }
        };
        (
            status,
            error,
            cache.base as i32,
            cache.limit as i32,
            i32::from(cache.selector),
            i32::from(cache.usable),
            i32::from(cache.readable),
            i32::from(cache.writable),
            i32::from(cache.executable),
            i32::from(cache.expand_down),
            i32::from(cache.big),
            0, // Host-managed code/data descriptors are pre-accessed.
        )
    });
    let query = FunctionRef::new(&mut context, move |_, (selector,)| {
        tables
            .lock()
            .unwrap()
            .query_user_segment_descriptor(selector)
    });
    let complete = FunctionRef::new(&mut context, |_, (_selector,)| ());
    [
        resolve.import(abi::RESOLVE_SEGMENT),
        query.import(abi::QUERY_SEGMENT_DESCRIPTOR),
        complete.import(abi::COMPLETE_SEGMENT_LOAD),
    ]
}
