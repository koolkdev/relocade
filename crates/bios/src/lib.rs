//! BIOS services for a real-mode PC, reached through ordinary runtime hooks.
//!
//! INT 10h sets text mode 03h with output, cursors, pages and scrolling.
//! Graphics modes 04h–06h, 0Dh, 12h and 13h program VGA for direct guest pixel
//! and palette writes, including CGA-compatible packed storage at B8000.
//! Mode setting honors AL bit 7 to preserve video memory; AH=0Fh reports the
//! mode, that flag, character columns and active page. AH=0Ch/0Dh write/read
//! graphics pixels, with XOR in packed/planar modes and eight pages in mode 0Dh.
//! Graphics page/cursor and scroll services use that same layout. AH=09h/0Ah
//! draw characters from live BIOS font vectors; AH=0Eh and [`write_text`] share
//! teletype wrapping and scrolling in text and graphics modes.
//! AH=0Bh sets legacy colors through VGA attributes; AH=10h reads/writes
//! attribute and DAC palettes, blink, paging and the DAC mask.
//! AH=11h loads text fonts, selects font banks and reports ROM font pointers.
//! AH=12h/BL=30h selects the next text mode set's scanline profile.
//! Unsupported video services or pixel coordinates/pages return AH=86h with
//! carry set.
//! INT 16h/AH=00h/01h/02h reads, peeks and queries the shared host keyboard.
//! INT 1Ah/AH=00h reads ticks and midnight rollover from the BIOS data area at
//! 0040:006C and 0040:0070. A machine timer must update these fields.
#![forbid(unsafe_code)]

mod keyboard;
mod screen;
mod video;

pub use keyboard::{KeyStroke, Keyboard, ShiftState};
use relocade_machine::{real_mode_interrupt, CallContext, CpuView, Hook, HookResult, MemoryAccess};
use relocade_machine::{IoBus, MemoryView};
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
pub fn hooks<T: 'static>(keyboard: Keyboard) -> [Hook<T>; 3] {
    [
        Hook::new(CLOCK_ADDRESS, async |call| dispatch(call, clock)),
        Hook::new(KEYBOARD_ADDRESS, async move |call| {
            keyboard::interrupt(call, &keyboard).await
        }),
        Hook::new(VIDEO_ADDRESS, async |call| dispatch(call, video::interrupt)),
    ]
}

/// Synchronous BIOS services borrow CPU storage once and use retained resources.
struct BiosContext<'a> {
    cpu: CpuView<MemoryAccess<'a>>,
    memory: MemoryView<'a>,
    io: &'a IoBus,
}

fn dispatch<T>(
    mut call: CallContext<T>,
    handler: impl FnOnce(&mut BiosContext<'_>) -> HookResult,
) -> HookResult {
    call.cpu.with(|cpu| {
        handler(&mut BiosContext {
            cpu,
            memory: call.memory.view(),
            io: &call.io,
        })
    })
}

fn clock(call: &mut BiosContext<'_>) -> HookResult {
    if call.cpu.ah() == 0 {
        let mut data = [0; 5];
        call.memory
            .read(CLOCK_TICKS, &mut data)
            .expect("BIOS clock data");
        let cpu = &mut call.cpu;
        cpu.set_dx(u16::from_le_bytes([data[0], data[1]]));
        cpu.set_cx(u16::from_le_bytes([data[2], data[3]]));
        cpu.set_al(data[4]);
        call.memory
            .write(MIDNIGHT_ROLLOVER, &[0])
            .expect("BIOS midnight rollover");
    } else {
        // Unsupported BIOS function.
        call.cpu.set_ah(0x86);
        real_mode_interrupt::set_carry(&call.cpu, &mut call.memory, true);
    }
    real_mode_interrupt::return_from_interrupt(&mut call.cpu)
}
