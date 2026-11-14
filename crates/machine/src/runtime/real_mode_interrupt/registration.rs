use super::{hook, vector_address, write_vector, HOOK_STUB};
use crate::{Cpu, CpuContext, FarPointer, Hook, Result};
use relocade_x86::{Exception, PhysicalMapping};

// Entry, resume and the ordinary far return interpreted on completion.
const FAR_PROCEDURE_STUB: [u8; 3] = [0x90, 0x90, 0xcb];

/// Registers real-mode interrupt handlers and far procedures on one stopped CPU.
///
/// Interrupt registration writes the IVT destination; both kinds initialize a guest stub.
/// [`Self::install`] publishes the handlers together. Guest INT instructions still
/// follow the IVT, so guest code can replace a vector and chain to its old target.
pub struct Interrupts<'cpu> {
    cpu: &'cpu mut Cpu,
    hooks: Vec<Hook>,
}

impl<'cpu> Interrupts<'cpu> {
    /// Begins registration while no host service is pending or suspended.
    pub fn new(cpu: &'cpu mut Cpu) -> Self {
        assert!(
            cpu.suspended.is_empty(),
            "host calls are parked; complete or abandon them before registering interrupts"
        );
        cpu.instance_mut();
        Self {
            cpu,
            hooks: Vec::new(),
        }
    }

    /// Binds a vector and guest entry address to an ordinary async handler.
    ///
    /// The IVT must be writable RAM. The entry must have room for the three-byte
    /// stub within its code segment and be backed by RAM or ROM. ROM initialization
    /// follows the memory map, including nonidentity backing offsets and A20.
    /// Handlers borrow CPU state, mapped memory and I/O from this CPU's resources.
    /// This does not change interrupt-enable behavior.
    /// Use [`super::interrupt_handler!`] to register a service annotated with
    /// [`super::interrupt`], applying its register arguments and return mappings.
    pub fn register_interrupt<F>(&mut self, vector: u8, entry: FarPointer, handler: F)
    where
        F: AsyncFn(&mut CpuContext<'_>) + 'static,
    {
        self.register_vector(vector, entry);
        self.register_entry(entry, &HOOK_STUB, handler);
    }

    /// Binds an interrupt to a Rust handler that completes through a far return.
    ///
    /// The saved FLAGS word stays on the caller's stack. The handler returns
    /// status through live flags; saved-FLAGS mappings such as `cf` must not be used.
    /// Entry requirements and suspension are the same as [`Self::register_interrupt`].
    pub fn register_interrupt_with_far_return<F>(
        &mut self,
        vector: u8,
        entry: FarPointer,
        handler: F,
    ) where
        F: AsyncFn(&mut CpuContext<'_>) + 'static,
    {
        self.register_vector(vector, entry);
        self.register_far_procedure(entry, handler);
    }

    fn register_vector(&mut self, vector: u8, entry: FarPointer) {
        let memory = self.cpu.memory();
        let vector_address = vector_address(vector).address();
        assert!(
            matches!(
                memory.configuration().memory_map().get(vector_address),
                PhysicalMapping::Ram { .. }
            ),
            "interrupt vector table must be writable RAM"
        );
        write_vector(&mut memory.view(), vector, entry);
    }

    /// Binds a far procedure to an ordinary async handler without changing the IVT.
    /// The handler shares interrupt suspension and guest-call resumption. Normal
    /// completion executes RETF; argument cleanup belongs to the caller. This
    /// entry has no saved interrupt FLAGS and does not change live IF.
    /// The three-byte stub has the same memory requirements as an interrupt entry.
    pub fn register_far_procedure<F>(&mut self, entry: FarPointer, handler: F)
    where
        F: AsyncFn(&mut CpuContext<'_>) + 'static,
    {
        self.register_entry(entry, &FAR_PROCEDURE_STUB, handler);
    }

    fn register_entry<F>(&mut self, entry: FarPointer, stub: &[u8; 3], handler: F)
    where
        F: AsyncFn(&mut CpuContext<'_>) + 'static,
    {
        assert!(
            usize::from(entry.offset) + stub.len() <= 0x10000,
            "service stub must fit in its code segment"
        );
        let memory = self.cpu.memory();
        {
            let mut view = memory.view();
            for (offset, &byte) in stub.iter().enumerate() {
                let address = entry.address() + offset as u32;
                let backing = match memory.configuration().memory_map().get(address) {
                    PhysicalMapping::Ram { backing_offset }
                    | PhysicalMapping::Rom { backing_offset } => backing_offset,
                    _ => panic!("service stub must be backed by RAM or ROM"),
                };
                view.write_backing(backing, &[byte])
                    .expect("service stub backing");
            }
        }

        let cpu = self.cpu.cpu_state();
        let memory = memory.with_io(self.cpu.instance.io.clone());
        let guest_calls = self.cpu.guest_calls.clone();
        self.hooks
            .extend(hook(entry.packed(), cpu.clone(), async move || {
                handler(&mut CpuContext {
                    cpu: &cpu,
                    memory: memory.view(),
                    io: memory.io(),
                    guest_calls: Some(&guest_calls),
                })
                .await
            }));
    }

    /// Installs a default handler that returns this exception to the host.
    /// Guest IVT replacement works normally. The live CPU is already in the
    /// handler when it reports the exception; the caller remains in its saved frame.
    pub fn register_unhandled_exception(&mut self, exception: Exception, entry: FarPointer) {
        let exit = self.cpu.exit_handle();
        self.register_interrupt(exception.vector() as u8, entry, async move |_| {
            exit.request_unhandled_exception(exception);
        });
    }

    /// Publishes the registered handlers through the CPU's ordinary hook installer.
    pub async fn install(self) -> Result<()> {
        self.cpu.install(self.hooks).await
    }
}
