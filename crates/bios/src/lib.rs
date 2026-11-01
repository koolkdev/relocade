//! BIOS services for a real-mode PC, reached through ordinary runtime hooks.
//!
//! INT 10h sets text modes 00h–03h and 07h with output, cursors, pages and scrolling.
//! Mode 07h uses the VGA monochrome palette, nine-dot cells and B0000 aperture.
//! Graphics modes 04h–06h and 0Dh–13h program VGA for direct guest pixel
//! and palette writes, including CGA-compatible packed storage at B8000.
//! Mode setting honors AL bit 7 to preserve video memory; AH=0Fh reports the
//! mode, that flag, character columns and active page. AH=0Ch/0Dh write/read
//! graphics pixels, with XOR in packed/planar modes and eight, four or two pages
//! in modes 0Dh, 0Eh or 0Fh/10h respectively.
//! Graphics page/cursor and scroll services use that same layout. AH=09h/0Ah
//! draw characters from live BIOS font vectors; AH=0Eh and [`write_text`] share
//! teletype wrapping and scrolling in text and graphics modes.
//! AH=13h writes strings with fixed or inline attributes to a requested page,
//! sharing those controls and optionally updating the stored cursor.
//! AH=0Bh sets legacy colors through VGA attributes; AH=10h reads/writes
//! attribute and DAC palettes, blink, paging and the DAC mask.
//! AX=101Bh converts a DAC range to grayscale using IBM VGA's fixed-point weights.
//! AH=11h loads text fonts, selects font banks and reports ROM font pointers.
//! Its AL=20h–24h services select graphics font vectors and BIOS character geometry.
//! AH=12h/BL=30h selects the next text mode set's scanline profile.
//! BL=31h controls default palette loading; BL=33h controls grayscale conversion
//! for mode palettes and explicit BIOS DAC writes.
//! AH=12h/BL=10h reports adapter configuration; AX=1A00h/1A01h get/set the
//! BIOS display-combination description without changing hardware or rendering.
//! AH=1Bh/BX=0 writes the functionality/state report to ES:DI; other BX values
//! return AL=0 without writing the buffer. Other unsupported video services or
//! pixel coordinates/pages return AH=86h with carry set.
//! INT 16h/AH=00h/01h/02h reads, peeks and queries the shared BIOS keyboard state.
//! INT 1Ah/AH=00h reads ticks and midnight rollover from the BIOS data area at
//! 0040:006C and 0040:0070. A machine timer must update these fields.
#![forbid(unsafe_code)]

mod keyboard;
pub mod screen;
mod video;

pub use keyboard::{KeyStroke, Keyboard, ShiftState};
use relocade_machine::{real_mode_interrupt, Hook};
use relocade_machine::{CpuStateAccess, IoBus, MemoryBus, MemoryView};
pub use screen::write_text;

const BIOS_SEGMENT: u16 = 0xf000;
const CLOCK_ENTRY: u16 = 0xff00;
const CLOCK_ADDRESS: u32 = ((BIOS_SEGMENT as u32) << 4) + CLOCK_ENTRY as u32;
const KEYBOARD_ENTRY: u16 = 0xff10;
const KEYBOARD_ADDRESS: u32 = ((BIOS_SEGMENT as u32) << 4) + KEYBOARD_ENTRY as u32;
const VIDEO_ENTRY: u16 = 0xff20;
const VIDEO_ADDRESS: u32 = ((BIOS_SEGMENT as u32) << 4) + VIDEO_ENTRY as u32;
const CLOCK_TICKS: u32 = 0x46c;
const MIDNIGHT_ROLLOVER: u32 = 0x470;

/// Initializes clock/video data, BIOS vectors, ROM fonts and hook instructions.
/// The machine must map low memory as RAM and F0000..FFFFF as identity-backed ROM.
/// Map [`relocade_vga::MEMORY_RANGE`] as MMIO and connect VGA to that memory
/// range and [`relocade_vga::PORT_RANGES`] on the supplied bus. Font loading uses
/// the A0000 aperture before mode setup restores the color text aperture.
pub fn initialize(memory: &mut MemoryView<'_>, io: &IoBus) {
    video::initialize(memory, io);
    memory.write(CLOCK_TICKS, &[0; 5]).expect("BIOS clock data");
    let [segment_lo, segment_hi] = BIOS_SEGMENT.to_le_bytes();
    for (vector, offset) in [
        (0x1a, CLOCK_ENTRY),
        (0x16, KEYBOARD_ENTRY),
        (0x10, VIDEO_ENTRY),
    ] {
        let [offset_lo, offset_hi] = offset.to_le_bytes();
        memory
            .write(vector * 4, &[offset_lo, offset_hi, segment_lo, segment_hi])
            .expect("BIOS interrupt vector");
        let address = ((BIOS_SEGMENT as u32) << 4) + u32::from(offset);
        memory
            .write_backing(address, &real_mode_interrupt::HOOK_STUB)
            .expect("BIOS ROM backing");
    }
}

/// Returns hooks for installation alongside another environment's hooks.
/// The keyboard is captured by the hooks and may be shared with console input.
pub fn hooks(cpu: CpuStateAccess, memory: MemoryBus, keyboard: Keyboard) -> [Hook; 3] {
    [
        hook(CLOCK_ADDRESS, cpu.clone(), memory.clone(), clock),
        hook(
            KEYBOARD_ADDRESS,
            cpu.clone(),
            memory.clone(),
            async move |call| keyboard::interrupt(call, &keyboard).await,
        ),
        hook(VIDEO_ADDRESS, cpu, memory, video::interrupt),
    ]
}

struct BiosContext<'call> {
    cpu: &'call CpuStateAccess,
    memory: MemoryView<'call>,
    io: &'call IoBus,
}

fn hook<F>(address: u32, cpu: CpuStateAccess, memory: MemoryBus, handler: F) -> Hook
where
    F: AsyncFn(&mut BiosContext<'_>) + 'static,
{
    real_mode_interrupt::hook(address, cpu.clone(), async move || {
        handler(&mut BiosContext {
            cpu: &cpu,
            memory: memory.view(),
            io: memory.io(),
        })
        .await
    })
}

async fn clock(call: &mut crate::BiosContext<'_>) {
    if call.cpu.ah() == 0 {
        let mut data = [0; 5];
        call.memory
            .read(CLOCK_TICKS, &mut data)
            .expect("BIOS clock data");
        let cpu = call.cpu;
        cpu.set_dx(u16::from_le_bytes([data[0], data[1]]));
        cpu.set_cx(u16::from_le_bytes([data[2], data[3]]));
        cpu.set_al(data[4]);
        call.memory
            .write(MIDNIGHT_ROLLOVER, &[0])
            .expect("BIOS midnight rollover");
    } else {
        // Unsupported BIOS function.
        call.cpu.set_ah(0x86);
        real_mode_interrupt::set_carry(call.cpu, &mut call.memory, true);
    }
}
