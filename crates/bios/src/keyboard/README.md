# BIOS keyboard services

`Keyboard::new(memory)` initializes the keyboard data for a session. The default
INT 09h handler reads one byte from the AT controller, updates the BIOS flags,
translates the US layout and appends a word to the guest ring, then sends PIC EOI.
A guest may replace that vector to consume port 60h itself or chain the original
handler. Physical input does not enter the BIOS queue by any other route.

INT 16h/AH=00h, 01h, 02h and 05h and DOS console reads use the same BIOS data area:

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
Pause and Print Screen are consumed without producing characters; their BIOS
pause/print actions, Ctrl-Break/Alt-Delete actions, Alt-numpad composition, BIOS
LED synchronization and enhanced INT 16h functions are not implemented. The ring
currently stores normalized zero/scan-code pairs for special keys, including
F11/F12; it does not yet preserve the enhanced BIOS E0/F0 markers or distinguish
legacy AH=00h/01h filtering from enhanced AH=10h/11h reads.

Set-1 bytes decode to named keys before US translation. Function and navigation
keys have explicit BIOS scan-code columns for unmodified, Shift, Ctrl and Alt;
these codes are not offsets from hardware scan codes. Ctrl combinations without
a BIOS mapping produce no key. Alt-numpad digits are ignored until composition
is implemented. BDA flag types separately name active shifts/toggles, held keys,
and enhanced-keyboard status, preserving guest-written bits during updates.

The data layout and scan translation follow the keyboard BIOS listings in the
[IBM PC/AT Technical Reference, March 1986](https://ftpmirror.your.org/pub/misc/bitsavers/pdf/ibm/pc/at/6183355_PC_AT_Technical_Reference_Mar86.pdf).
`data`, `scan` and `layout` keep guest storage, IRQ decoding and character mapping
separate; the public handle provides BIOS buffer operations and pending-read wakeups.

The native Rust prompt uses `PhysicalKeyboard` to track its own modifier state
and call the same `layout` translation. It does not write this guest data area
or execute IRQ handlers. The shared key table maps both physical identities
and set-1 bytes, keeping host-language text out of either path.
