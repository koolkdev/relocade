use super::*;
use crate::tests::support::{self, Fixture};
use relocade_x86::{
    Compiler, CpuState, ExecutionContext, ExecutionProfile, PhysicalMemoryMap, Segment, Segments,
};

#[test]
fn callbacks_apply_compatible_segment_edits() {
    let model =
        relocade_x86::CpuModel::default().with_feature(relocade_x86::CpuFeature::Cmov, false);
    let cpu = CpuState {
        segments: Segments::real_mode(),
        ..CpuState::default()
    };
    let config = support::configuration(
        Compiler::new(ExecutionProfile::Real16)
            .with_cpu_model(model)
            .target(),
        cpu,
        0,
        PhysicalMemoryMap::new([]).unwrap(),
    )
    .unwrap();
    let mut fixture = Fixture::new(&config.cpu, &config.machine);
    let mut state = RuntimeState::<()>::default();
    {
        let mut call = HostCall::new(fixture.view(&config), &mut state);
        call.cpu().set_ax(42);
        call.cpu().set_segment(
            Segment::Cs,
            relocade_x86::SegmentRegister::real_mode(Segment::Cs, 0x1000),
        );
    }
    let mut expected = cpu;
    expected.registers.eax = 42;
    expected.segments.cs = relocade_x86::SegmentRegister::real_mode(Segment::Cs, 0x1000);
    assert_eq!(
        fixture.context,
        ExecutionContext {
            cpu: expected,
            instruction_target: 998,
            cpu_model: model,
        }
        .to_bytes(),
    );
}
