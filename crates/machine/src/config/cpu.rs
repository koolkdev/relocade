use super::ConfigurationError;
use relocade_x86::{
    CompilerConfig, CpuMode, CpuModel, CpuState, DescriptorTableRegister, EntryAssumptions,
    MemoryBackend, PrivilegeLevel, Segment, SegmentRegister, SystemSegmentRegister,
};

/// Validated initial state and execution policy for one CPU.
#[derive(Clone, Copy)]
pub struct CpuConfig {
    pub(crate) execution: CpuExecution,
    cpu: CpuState,
}

#[derive(Clone, Copy)]
pub(crate) enum CpuExecution {
    Pc(CompilerConfig),
    Fixed(CompilerConfig),
}

impl CpuConfig {
    /// Configures a physical PC CPU with the default model and instruction budgets.
    /// Its interpreter accepts canonical real mode and protected ring 3, with
    /// paging disabled in both modes. Its JIT currently compiles real-mode code.
    pub fn pc(cpu: CpuState) -> Result<Self, ConfigurationError> {
        Self::configured(
            CpuExecution::Pc(CompilerConfig::new(
                EntryAssumptions::REAL_MODE,
                MemoryBackend::PhysicalBus,
            )),
            cpu,
        )
    }

    /// Requires the supplied entry assumptions on initialization and restore.
    pub fn new(config: CompilerConfig, cpu: CpuState) -> Result<Self, ConfigurationError> {
        Self::configured(CpuExecution::Fixed(config), cpu)
    }

    fn configured(execution: CpuExecution, cpu: CpuState) -> Result<Self, ConfigurationError> {
        execution.validate_cpu(&cpu)?;
        Ok(Self { execution, cpu })
    }

    /// Selects the processor features exposed to guest code.
    #[must_use]
    pub fn with_cpu_model(mut self, model: CpuModel) -> Self {
        self.execution.config_mut().cpu_model = model;
        self
    }

    /// Selects whether the scheduler can arm instruction budgets.
    #[must_use]
    pub fn with_instruction_budgets(mut self, enabled: bool) -> Self {
        self.execution.config_mut().instruction_budgets = enabled;
        self
    }

    /// Inputs used to compile guest code. Read the CPU image for its live mode and width.
    pub fn compiler_config(&self) -> CompilerConfig {
        self.execution.config()
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

impl CpuExecution {
    pub(crate) fn config(self) -> CompilerConfig {
        match self {
            Self::Pc(config) | Self::Fixed(config) => config,
        }
    }

    fn config_mut(&mut self) -> &mut CompilerConfig {
        match self {
            Self::Pc(config) | Self::Fixed(config) => config,
        }
    }

    pub(crate) fn validate_cpu(self, cpu: &CpuState) -> Result<(), ConfigurationError> {
        let mode = cpu.mode();
        let compatible = match self {
            Self::Pc(_) => {
                cpu.has_supported_control_state()
                    && Segment::ALL
                        .into_iter()
                        .all(|segment| self.accepts_segment(mode, segment, &cpu.segments[segment]))
            }
            Self::Fixed(config) => cpu.is_compatible_with(config),
        };
        if !compatible {
            return Err(ConfigurationError::IncompatibleCpu);
        }
        validate_idtr(mode, cpu.idtr)?;
        validate_system_segment(cpu.ldtr, true)?;
        validate_system_segment(cpu.tr, false)
    }

    pub(crate) fn accepts_segment(
        self,
        mode: CpuMode,
        segment: Segment,
        value: &SegmentRegister,
    ) -> bool {
        match self {
            Self::Fixed(config) => value.is_compatible_with(config.entry, segment),
            Self::Pc(_) => match mode {
                CpuMode::Real => *value == SegmentRegister::real_mode(segment, value.selector),
                CpuMode::Protected => {
                    segment != Segment::Cs
                        || mode.privilege_level(value.selector) == PrivilegeLevel::Ring3
                }
                CpuMode::Virtual8086 => false,
            },
        }
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
