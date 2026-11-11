use super::ConfigurationError;
use relocade_x86::{
    CpuMode, CpuState, DescriptorTableRegister, ExecutionTarget, SystemSegmentRegister,
};

/// Validated initial state and execution policy for one CPU.
#[derive(Clone, Copy)]
pub struct CpuConfig {
    target: ExecutionTarget,
    cpu: CpuState,
}

impl CpuConfig {
    /// Initializes architectural state while retaining the compatibility contract
    /// of today's generated code. The target does not override the CPU's mode.
    pub fn new(target: ExecutionTarget, cpu: CpuState) -> Result<Self, ConfigurationError> {
        validate_cpu(target, &cpu)?;
        Ok(Self { target, cpu })
    }

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
    if !cpu.is_compatible_with(target.profile) {
        return Err(ConfigurationError::IncompatibleCpu);
    }
    validate_idtr(cpu.mode(), cpu.idtr)?;
    validate_system_segment(cpu.ldtr, true)?;
    validate_system_segment(cpu.tr, false)
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
