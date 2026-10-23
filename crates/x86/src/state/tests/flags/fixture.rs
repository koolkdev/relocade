use crate::CpuState;

pub(super) fn initial_cpu() -> CpuState {
    let mut cpu = CpuState::filled(0xa5);
    cpu.flags.status_source.kind = 9;
    cpu.flags.status_source.left = 7;
    cpu.flags.status_source.right = 8;
    cpu.eip = 0x1000;
    cpu.instruction_count = u64::MAX;
    cpu
}
