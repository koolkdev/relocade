# BIOS keyboard buffer

INT 16h/AH=00h, 01h, 02h and 05h and DOS console reads use the same BIOS data area:

| Location | Meaning |
| --- | --- |
| 0040:0017–0018 | Modifier, toggle and held-lock flags |
| 0040:001A–001C | Ring head and tail offsets relative to segment 0040 |
| 0040:001E–003D | Default ring, with fifteen usable keystroke slots |
| 0040:0080–0082 | Ring start and end offsets |
| 0040:0096 | Enhanced keyboard, right Ctrl/Alt and prefix flags |

BIOS services read guest pointers, words and flags directly from RAM. The guest
RAM ring is the only keystroke queue. Physical input drops a character when full.
`Keyboard::store_key` implements INT 16h/AH=05h: store the character/scan-code word
from CX, returning AL=0 on success or AL=1
when full. Host callers use the same operation; it does not manufacture hardware
IRQs or controller bytes. Reading or peeking never refills the ring from elsewhere.

The `Write` adapter stores bytes with scan code zero. It reports the accepted
count, or `WouldBlock` when the ring has no room. Script producers retain the
unwritten suffix and let the guest consume input before retrying; there is no
BIOS-side backlog. A guest flush therefore empties all BIOS keystrokes.

`DataArea` owns the guest RAM layout, ring operations and modifier flag bytes.
