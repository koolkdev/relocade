//! BIOS hook registration and shared interrupt return handling.

use crate::{
    clock, keyboard, video, Keyboard, CLOCK_ADDRESS, KEYBOARD_ADDRESS, TIMER_ADDRESS, VIDEO_ADDRESS,
};
use relocade_machine::{real_mode_interrupt, CpuStateAccess, Hook, MemoryBus};

/// Returns hooks for installation alongside another environment's hooks.
/// The keyboard is captured by the hooks and may be shared with console input.
/// INT 10h, 16h and 1Ah enable live IF during service execution. Ready services
/// finish in one poll; pending services allow the driver to deliver device IRQs.
pub fn hooks(
    cpu: CpuStateAccess,
    memory: MemoryBus,
    keyboard: Keyboard,
) -> impl Iterator<Item = Hook> {
    [
        hook(
            TIMER_ADDRESS,
            cpu.clone(),
            memory.clone(),
            clock::tick_entry,
        ),
        hook(
            CLOCK_ADDRESS,
            cpu.clone(),
            memory.clone(),
            clock::dispatch_entry,
        ),
        hook(
            KEYBOARD_ADDRESS,
            cpu.clone(),
            memory.clone(),
            async move |call| {
                keyboard::dispatch_entry(call, &keyboard).await;
            },
        ),
        hook(VIDEO_ADDRESS, cpu, memory, video::dispatch_entry),
    ]
    .into_iter()
    .flatten()
}

pub(crate) use relocade_machine::CpuContext as BiosContext;

fn hook<F>(address: u32, cpu: CpuStateAccess, memory: MemoryBus, handler: F) -> [Hook; 3]
where
    F: AsyncFn(&mut BiosContext<'_>) + 'static,
{
    real_mode_interrupt::hook(address, cpu.clone(), async move || {
        let mut call = BiosContext {
            cpu: &cpu,
            memory: memory.view(),
            io: memory.io(),
        };
        real_mode_interrupt::with_interrupts_enabled(&cpu, handler(&mut call)).await
    })
}

#[real_mode_interrupt::interrupt(returns { status => { ah = status, cf = true } })]
pub(crate) fn unsupported() -> u8 {
    0x86
}
