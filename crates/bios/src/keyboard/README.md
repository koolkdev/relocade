# BIOS keyboard services

`Keyboard::new(memory)` initializes the keyboard data for a session. The default
INT 09h handler reads one byte from the AT controller's output latch, updates the
BIOS flags, translates the US layout and appends a word to the guest ring, then
sends PIC EOI.
A guest may replace that vector to consume port 60h itself or chain the original
handler. Like the IBM AT BIOS (KYBD listing, page 5-129), the handler reads port 60h
without checking OBF: a guest hook may already have read the byte and cleared OBF
before chaining. The handler inhibits the interface with ADh and awaits command
acceptance before reading. It sends PIC EOI with IF clear, then releases the
interface with AEh before IRET. Port-status waits sleep on device notifications.
Readiness and acceptance share a 500 ms per-command deadline on the PC clock,
a nominal bound for the AT BIOS's CPU-dependent polling loops. On timeout, BIOS
still reads the latch, completes EOI, attempts to enable the interface within
another bounded command, and returns through IRET.
Physical input does not enter the BIOS queue by any other route.

INT 16h/AH=00h/01h/02h/05h/10h/11h/12h and DOS console reads use the same BIOS data area:

| Location | Meaning |
| --- | --- |
| 0040:0017–0018 | Modifier, toggle and held-lock flags |
| 0040:001A–001C | Ring head and tail offsets relative to segment 0040 |
| 0040:001E–003D | Default ring, with fifteen usable keystroke slots |
| 0040:0080–0082 | Ring start and end offsets |
| 0040:0096 | Enhanced keyboard, right Ctrl/Alt and prefix flags |

Guest edits to pointers, words and flags take effect on the next service or IRQ.
The guest RAM ring is the only keystroke queue. The default IRQ handler drops a
character when it is full. `Keyboard::store_key` implements INT 16h/AH=05h:
store the character/scan-code word from CX, returning AL=0 on success or AL=1
when full. Host callers use the same operation; it does not manufacture hardware
IRQs or controller bytes. Reading or peeking never refills the ring from elsewhere.

The `Write` adapter stores bytes with scan code zero. It reports the accepted
count, or `WouldBlock` when the ring has no room. Script producers retain the
unwritten suffix and let the guest consume input before retrying; there is no
BIOS-side backlog. A guest flush therefore empties all BIOS keystrokes.

The decoder ignores key breaks as characters, distinguishes modifier sides,
suppresses repeated lock toggles and ignores the fake shifts in Print Screen.
NumLock/Shift select keypad numbers, independently of dedicated navigation keys.
Ctrl-Break clears the ring and sets the break flag at `0040:0071`, then awaits
`INT 1Bh` and inserts a zero keystroke on return. The IRQ wrapper owns PIC completion.

Pause and Print Screen are consumed without producing characters; their BIOS
pause/print actions, Ctrl-Alt-Delete, Alt-numpad composition and BIOS
LED synchronization are not implemented. The ring retains enhanced BIOS E0/F0
markers. Legacy INT 16h/AH=00h/01h translates compatible keys and discards
enhanced-only entries, including during status queries. Enhanced AH=10h/11h
preserves extended identities and normalizes F0 markers. Both interfaces leave
scan-zero OEM characters intact. AH=12h reports left/right modifier and held-key
state from the BDA. Host reads select the same behavior with `KeyMode`.

Set-1 bytes decode to named keys before US translation. Function and navigation
keys have explicit BIOS scan-code columns for unmodified, Shift, Ctrl and Alt;
these codes are not offsets from hardware scan codes. Ctrl combinations without
a BIOS mapping produce no key. Alt-numpad digits are ignored until composition
is implemented. BDA flag types separately name active shifts/toggles, held keys,
and enhanced-keyboard status, preserving guest-written bits during updates.

The data layout and scan translation follow the keyboard BIOS listings in the
[IBM PC/AT Technical Reference, March 1986](https://ftpmirror.your.org/pub/misc/bitsavers/pdf/ibm/pc/at/6183355_PC_AT_Technical_Reference_Mar86.pdf).
`data`, `scan`, `layout` and `encoding` keep guest storage, IRQ decoding, character
mapping and INT 16h compatibility translation separate; the public handle provides
BIOS buffer operations and pending-read wakeups.

The native Rust prompt uses `PhysicalKeyboard` to track its own modifier state
and call the same `layout` translation. It does not write this guest data area
or execute IRQ handlers. The shared key table maps both physical identities
and set-1 bytes, keeping host-language text out of either path. Native input and
the DOS console consume enhanced keys, converting navigation E0 markers into DOS
zero/scan-code byte pairs. The DOS policy follows the enhanced-keyboard selection
and E0 handling in [Microsoft MSINIT.ASM](https://github.com/microsoft/MS-DOS/blob/main/v4.0/src/BIOS/MSINIT.ASM#L752)
and [MSCON.ASM](https://github.com/microsoft/MS-DOS/blob/main/v4.0/src/BIOS/MSCON.ASM#L54).
