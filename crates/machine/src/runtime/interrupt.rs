//! Attaches the device/controller that supplies this CPU's interrupt requests.

use super::Cpu;
use crate::{ConfigurationError, InterruptSource};
use relocade_x86::ExecutionProfile;

impl<T: 'static> Cpu<T> {
    /// Connects or replaces the source of maskable interrupt vectors.
    /// The source retains requests while IF is clear or a protected instruction
    /// must execute first. A shared device can be passed as `Rc<RefCell<D>>`.
    /// Protected-mode and nonmaskable interrupt delivery are not implemented.
    ///
    /// # Panics
    /// Panics if a host service is suspended; complete it before replacing sources.
    pub fn set_interrupt_source(
        &mut self,
        source: impl InterruptSource + 'static,
    ) -> Result<(), ConfigurationError> {
        let machine = self.execution.machine_mut();
        if machine.context().cpu.execution_target().profile != ExecutionProfile::Real16 {
            return Err(ConfigurationError::InterruptsRequireRealMode);
        }
        machine.interrupts = Some(Box::new(source));
        Ok(())
    }
}
