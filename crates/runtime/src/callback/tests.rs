use super::*;
use relocade_wasm_runtime::WasmMemoryAccess;
use relocade_x86::{
    Compiler, CpuState, ExecutionContext, ExecutionProfile, PhysicalMemoryMap, Segment, Segments,
};
use std::{cell::Cell, sync::Arc};

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
fn callbacks_apply_compatible_segment_edits() {
    let model =
        relocade_x86::CpuModel::default().with_feature(relocade_x86::CpuFeature::Cmov, false);
    let cpu = CpuState {
        segments: Segments::real_mode(),
        ..CpuState::default()
    };
    let config = MachineConfig::new(
        Compiler::new(ExecutionProfile::Real16)
            .with_cpu_model(model)
            .target(),
        cpu,
        0,
        PhysicalMemoryMap::new([]).unwrap(),
    )
    .unwrap();
    let memory = RuntimeMemory {
        config: Arc::new(config),
        context: (),
        guest: (),
        mapping: (),
    };
    let mut caller = Caller {
        context: ExecutionContext {
            cpu,
            instruction_target: 998,
            cpu_model: model,
        }
        .to_bytes(),
        reads: Cell::new(0),
        state: RuntimeState::default(),
    };

    let value = memory.with_host_call(&mut caller, |call| {
        call.cpu().set_ax(41);
        17
    });
    assert_eq!(value, 17);
    assert_eq!(caller.reads.get(), 0);

    memory.with_host_call(&mut caller, |call| {
        call.cpu().set_ax(42);
        call.cpu().set_segment(
            Segment::Cs,
            relocade_x86::SegmentRegister::real_mode(Segment::Cs, 0x1000),
        );
    });
    let mut expected = cpu;
    expected.registers.eax = 42;
    expected.segments.cs = relocade_x86::SegmentRegister::real_mode(Segment::Cs, 0x1000);
    assert_eq!(
        caller.context,
        ExecutionContext {
            cpu: expected,
            instruction_target: 998,
            cpu_model: model
        }
        .to_bytes()
    );
}
