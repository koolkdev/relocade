//! Protected-mode callbacks for a CPU with loaded segment caches and no descriptor tables.

use relocade_wasm_runtime::{FunctionRef, Import, WasmBackend, WasmRuntime};
use relocade_x86::{abi, Descriptor, Exception, Segment, SegmentRegister};

pub(super) fn imports(wasm: &mut WasmRuntime) -> [Import<'static, WasmRuntime>; 3] {
    // CpuConfig supplies already-loaded caches. Until an embedding installs
    // descriptor tables, every slot is empty, including the initial CS/SS slots.
    let empty = Descriptor::default();
    let mut context = wasm.context();
    let resolve = FunctionRef::new(&mut context, move |_, (segment, selector): (i32, i32)| {
        let destination = Segment::ALL[segment as usize];
        let (status, error, cache) = match empty.resolve_user_segment(destination, selector as u16)
        {
            Ok(cache) => (0, 0, cache),
            Err(fault @ Exception::GeneralProtection { error_code }) => (
                fault.vector() as i32,
                error_code as i32,
                SegmentRegister::default(),
            ),
            Err(_) => unreachable!("an empty descriptor can only raise #GP"),
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
            0, // No accessed bit needs to be written.
        )
    });
    let query = FunctionRef::new(&mut context, move |_, (_selector,)| empty.query_user());
    let complete = FunctionRef::new(&mut context, |_, (_selector,)| ());
    [
        resolve.import(abi::RESOLVE_SEGMENT),
        query.import(abi::QUERY_SEGMENT_DESCRIPTOR),
        complete.import(abi::COMPLETE_SEGMENT_LOAD),
    ]
}
