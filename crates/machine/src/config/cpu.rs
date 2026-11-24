use super::ConfigurationError;
use relocade_x86::{
    CpuMode, CpuState, DescriptorTableRegister, ExecutionProfile, ExecutionTarget, MemoryBackend,
    PrivilegeLevel, Segment, SegmentRegister, SystemSegmentRegister,
};

/// Validated initial state and execution policy for one CPU.
#[derive(Clone, Copy)]
pub struct CpuConfig {
    target: ExecutionTarget,
    cpu: CpuState,
}

impl CpuConfig {
    /// Configures a physical PC CPU with the default model and instruction budgets.
    /// Its interpreter accepts canonical real mode and protected ring 3, with
    /// paging disabled in both modes. Its JIT currently compiles real-mode code.
    pub fn pc(cpu: CpuState) -> Result<Self, ConfigurationError> {
        Self::new(ExecutionProfile::Real16.into(), cpu)
    }

    /// Configures JIT compilation and validates the initial CPU image. On the
    /// physical PC, a real-mode JIT target also allows interpreted protected user code.
    /// Other targets retain their fixed profile requirements.
    pub fn new(target: ExecutionTarget, cpu: CpuState) -> Result<Self, ConfigurationError> {
        validate_cpu(target, &cpu)?;
        Ok(Self { target, cpu })
    }

    /// The target used to compile JIT code. Read the CPU image for its live mode and width.
    pub fn execution_target(&self) -> ExecutionTarget {
        self.target
    }

    pub fn cpu(&self) -> &CpuState {
        &self.cpu
    }
}

impl crate::worker::Transfer for CpuConfig {
    type State = Self;
    type Payload = ();

    fn encode(self) -> (Self, ()) {
        (self, ())
    }

    fn decode(state: Self, (): ()) -> Self {
        state
    }
}

pub(crate) fn validate_cpu(
    target: ExecutionTarget,
    cpu: &CpuState,
) -> Result<(), ConfigurationError> {
    let mode = cpu.mode();
    let compatible = if is_pc_jit_target(target) {
        cpu.has_supported_control_state()
            && Segment::ALL
                .into_iter()
                .all(|segment| pc_accepts_segment(mode, segment, &cpu.segments[segment]))
    } else {
        cpu.is_compatible_with(target)
    };
    if !compatible {
        return Err(ConfigurationError::IncompatibleCpu);
    }
    validate_idtr(mode, cpu.idtr)?;
    validate_system_segment(cpu.ldtr, true)?;
    validate_system_segment(cpu.tr, false)
}

pub(crate) fn is_pc_jit_target(target: ExecutionTarget) -> bool {
    target.profile == ExecutionProfile::Real16
        && target.memory_backend == MemoryBackend::PhysicalBus
}

pub(crate) fn pc_accepts_segment(mode: CpuMode, segment: Segment, value: &SegmentRegister) -> bool {
    match mode {
        CpuMode::Real => *value == SegmentRegister::real_mode(segment, value.selector),
        CpuMode::Protected => {
            segment != Segment::Cs || mode.privilege_level(value.selector) == PrivilegeLevel::Ring3
        }
        CpuMode::Virtual8086 => false,
    }
}

pub(crate) fn validate_idtr(
    mode: CpuMode,
    idtr: DescriptorTableRegister,
) -> Result<(), ConfigurationError> {
    if mode == CpuMode::Real && (idtr.base != 0 || idtr.limit != 0x3ff) {
        return Err(ConfigurationError::InvalidTableRegister);
    }
    Ok(())
}

pub(crate) fn validate_system_segment(
    segment: SystemSegmentRegister,
    ldt: bool,
) -> Result<(), ConfigurationError> {
    if segment.usable == 0 {
        return Ok(());
    }
    let kind = (segment.access_rights >> 8) & 15;
    let valid_type = if ldt {
        kind == 2
    } else {
        matches!(kind, 1 | 3 | 9 | 11)
    };
    if segment.selector & !3 == 0
        || segment.selector & 4 != 0
        || segment.access_rights & 0x9000 != 0x8000
        || !valid_type
    {
        return Err(ConfigurationError::InvalidTableRegister);
    }
    Ok(())
}
