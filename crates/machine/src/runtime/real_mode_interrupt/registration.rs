use super::{hook, HOOK_STUB};
use crate::{Cpu, CpuContext, FarPointer, Hook, Result};
use relocade_x86::PhysicalMapping;

/// Registers real-mode interrupt handlers on one stopped CPU.
///
/// Each registration writes the IVT destination and initializes its guest stub.
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
            !cpu.has_pending_host_calls(),
            "host calls are pending; complete or abandon them before registering interrupts"
        );
        cpu.machine_mut();
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
        assert!(
            usize::from(entry.offset) + HOOK_STUB.len() <= 0x10000,
            "interrupt stub must fit in its code segment"
        );
        let memory = self.cpu.memory();
        let vector_address = u32::from(vector) * 4;
        assert!(
            matches!(
                memory.configuration().memory_map().get(vector_address),
                PhysicalMapping::Ram { .. }
            ),
            "interrupt vector table must be writable RAM"
        );
        {
            let mut view = memory.view();
            for (offset, &byte) in HOOK_STUB.iter().enumerate() {
                let address = entry.address() + offset as u32;
                let backing = match memory.configuration().memory_map().get(address) {
                    PhysicalMapping::Ram { backing_offset }
                    | PhysicalMapping::Rom { backing_offset } => backing_offset,
                    _ => panic!("interrupt stub must be backed by RAM or ROM"),
                };
                view.write_backing(backing, &[byte])
                    .expect("interrupt stub backing");
            }
            view.write_u32(vector_address, entry.packed())
                .expect("interrupt vector");
        }

        let cpu = self.cpu.cpu_state();
        let memory = memory.with_io(self.cpu.machine.io.clone());
        self.hooks
            .extend(hook(entry.address(), cpu.clone(), async move || {
                handler(&mut CpuContext {
                    cpu: &cpu,
                    memory: memory.view(),
                    io: memory.io(),
                })
                .await
            }));
    }

    /// Publishes the registered handlers through the CPU's ordinary hook installer.
    pub async fn install(self) -> Result<()> {
        self.cpu.install(self.hooks).await
    }
}
