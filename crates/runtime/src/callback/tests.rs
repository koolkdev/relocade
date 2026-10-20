use super::*;
use crate::wasm::WasmMemoryAccess;
use relocade_machine::ConfigurationError;
use relocade_x86::{
    Compiler, CpuState, ExecutionContext, ExecutionProfile, PhysicalMemoryMap, Segment, Segments,
};
use std::cell::Cell;

struct Caller {
    context: [u8; ExecutionContext::BYTE_LEN],
    reads: Cell<usize>,
    state: RuntimeState,
}

impl WasmContext for Caller {
    type State = RuntimeState;

    fn state(&self) -> &RuntimeState {
        &self.state
    }

    fn state_mut(&mut self) -> &mut RuntimeState {
        &mut self.state
    }
}

impl WasmMemoryAccess for Caller {
    type Memory = ();

    fn read_memory(&self, _: &(), offset: u32, bytes: &mut [u8]) {
        self.reads.set(self.reads.get() + 1);
        bytes.copy_from_slice(&self.context[offset as usize..][..bytes.len()]);
    }

    fn write_memory(&mut self, _: &(), offset: u32, bytes: &[u8]) {
        self.context[offset as usize..][..bytes.len()].copy_from_slice(bytes);
    }
}

#[test]
fn callbacks_validate_only_segment_edits() {
    let cpu = CpuState {
        segments: Segments::real_mode(),
        ..CpuState::default()
    };
    let config = MachineConfig::new(
        Compiler::new(ExecutionProfile::Real16).target(),
        cpu,
        0,
        PhysicalMemoryMap::new([]).unwrap(),
    )
    .unwrap();
    let mut caller = Caller {
        context: ExecutionContext {
            cpu,
            instruction_target: 998,
        }
        .to_bytes(),
        reads: Cell::new(0),
        state: RuntimeState::default(),
    };

    let value = with_host_call(&config, &mut caller, (), (), |call| {
        call.cpu().set_ax(41);
        17
    });
    assert_eq!(value, 17);
    assert_eq!(caller.reads.get(), 0);

    let result = with_host_call(&config, &mut caller, (), (), |call| {
        call.cpu().set_ax(42);
        let mut cs = call.cpu().segment(Segment::Cs);
        cs.base = 1;
        call.cpu().set_segment(Segment::Cs, cs)
    });
    assert_eq!(result, Err(ConfigurationError::IncompatibleCpu));
    let mut expected = cpu;
    expected.registers.eax = 42;
    assert_eq!(
        caller.context,
        ExecutionContext {
            cpu: expected,
            instruction_target: 998
        }
        .to_bytes()
    );
}
