use crate::{Exception, Export, GuestMemory, ReturnValue};
use relocade_machine::{Cpu, Hook};
use relocade_x86::{ExecutionProfile, SegmentProfile};
use std::{borrow::Borrow, rc::Rc};

/// Builds the direct hooks for a group of DLL exports on a stopped Flat32 CPU.
/// Keep the loader's stub storage private until installation succeeds.
///
/// ```
/// use relocade_machine::Cpu;
/// use relocade_win32::{win32_export, Exception, Exports, GuestMemory};
/// use std::rc::Rc;
///
/// #[win32_export(name = "GetCurrentProcessId")]
/// fn process_id(#[context] id: &u32) -> u32 { *id }
///
/// async fn install(cpu: &mut Cpu, memory: Rc<dyn GuestMemory>, entry: u32,
///                  deliver: impl Fn(Exception) + 'static) -> Result<(), Box<dyn std::error::Error>> {
///     let mut exports = Exports::new(cpu, memory, deliver);
///     exports.bind::<process_id>(entry, 42u32)?;
///     exports.install().await?;
///     Ok(())
/// }
/// ```
pub struct Exports<'cpu> {
    cpu: &'cpu mut Cpu,
    memory: Rc<dyn GuestMemory>,
    deliver_exception: Rc<dyn Fn(Exception)>,
    hooks: Vec<Hook>,
}

impl<'cpu> Exports<'cpu> {
    /// The process supplies its virtual memory and exception policy once.
    /// On a service fault, EIP identifies the export. The exception handler
    /// transfers control to guest exception delivery or ends the execution run.
    ///
    /// # Panics
    /// Requires a machine dispatcher keyed by Flat32 EIP. Low-level Flat32
    /// embeddings can use [`Export::entry`] with their own dispatcher.
    pub fn new(
        cpu: &'cpu mut Cpu,
        memory: Rc<dyn GuestMemory>,
        deliver_exception: impl Fn(Exception) + 'static,
    ) -> Self {
        assert_eq!(
            cpu.cpu_state().execution_target().profile,
            ExecutionProfile::Protected(SegmentProfile::Flat32),
            "Win32 exports require a Flat32 dispatcher"
        );
        Self {
            cpu,
            memory,
            deliver_exception: Rc::new(deliver_exception),
            hooks: Vec::new(),
        }
    }

    /// Writes a stub in loader-owned memory and queues its entry, resume and
    /// near-return hooks. The import slot can point directly to this address.
    pub fn bind<E: Export>(
        &mut self,
        address: u32,
        context: impl Borrow<E::Context> + 'static,
    ) -> Result<(), Exception> {
        self.memory.write(address, E::SIGNATURE.stub_bytes())?;
        self.hook::<E>(address, context);
        Ok(())
    }

    pub(crate) fn hook<E: Export>(
        &mut self,
        address: u32,
        context: impl Borrow<E::Context> + 'static,
    ) {
        let state = self.cpu.cpu_state();
        let memory = self.memory.clone();
        let deliver_exception = self.deliver_exception.clone();
        self.hooks.extend(Hook::resumable(
            address,
            address.wrapping_add(1),
            address.wrapping_add(2),
            async move || {
                let entry = state.eip();
                state.set_eip(entry.wrapping_add(1));
                match E::entry(context.borrow(), memory.as_ref(), state.esp()).await {
                    Ok(value) => {
                        value
                            .into_registers()
                            .publish(|eax| state.set_eax(eax), |edx| state.set_edx(edx));
                        state.set_eip(state.eip().wrapping_add(1));
                    }
                    Err(fault) => {
                        state.set_eip(entry);
                        deliver_exception(fault);
                    }
                }
            },
        ));
    }

    /// Publishes all prepared hooks through the CPU's ordinary batch installer.
    pub async fn install(self) -> relocade_machine::Result<()> {
        self.cpu.install(self.hooks).await
    }
}
