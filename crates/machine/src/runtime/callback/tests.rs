use super::*;
use relocade_wasm_runtime::{
    Backend, MemoryAccess, SharedMemory, WasmBackend, WasmMemoryAccess, WasmSharedMemory,
};
use relocade_x86::{
    Compiler, CpuState, ExecutionContext, ExecutionProfile, PhysicalMemoryMap, Segment, Segments,
};
use std::{cell::Cell, sync::Arc};

struct Caller {
    context: SharedMemory,
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

    fn memory<'a>(&'a mut self, _: &'a ()) -> MemoryAccess<'a> {
        self.context.view()
    }

    fn read_memory(&self, _: &(), offset: u32, bytes: &mut [u8]) {
        self.reads.set(self.reads.get() + 1);
        self.context.read(offset, bytes);
    }

    fn write_memory(&mut self, _: &(), offset: u32, bytes: &[u8]) {
        self.context.write(offset, bytes);
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
    let config = CpuConfig::new(
        Compiler::new(ExecutionProfile::Real16)
            .with_cpu_model(model)
            .target(),
        cpu,
    )
    .unwrap();
    let machine_config = MachineConfig::new(0, PhysicalMemoryMap::new([]).unwrap()).unwrap();
    let memory = RuntimeMemory {
        config: Arc::new(config),
        machine_config: Arc::new(machine_config),
        context: (),
        guest: (),
        mapping: (),
    };
    let mut backend = Backend::new().unwrap();
    let allocation = backend
        .create_memory(relocade_compiler::abi::Memory {
            shared: true,
            maximum: Some(1),
            ..relocade_x86::abi::EXECUTION_CONTEXT
        })
        .unwrap();
    let context = backend.shared_memory(&allocation).unwrap();
    context.write(
        0,
        &ExecutionContext {
            cpu,
            instruction_target: 998,
            cpu_model: model,
        }
        .to_bytes(),
    );
    let mut caller = Caller {
        context,
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
    let mut actual = [0; ExecutionContext::BYTE_LEN];
    caller.context.read(0, &mut actual);
    assert_eq!(
        actual,
        ExecutionContext {
            cpu: expected,
            instruction_target: 998,
            cpu_model: model
        }
        .to_bytes()
    );
}
