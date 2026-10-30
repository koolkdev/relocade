# BIOS text fonts

The 8×14 and 8×16 fonts contain all 256 CP437 glyphs from **Modern DOS**
version 20190101.02 by Jayvee Enaguas, released under CC0 1.0.
The license is included in `modern-dos-LICENSE.txt`.
BIOS also uses the existing [8×8 font](../../vga/assets/README.md).

Sources: [ModernDOS8x14.ttf](https://github.com/susam/pcface/blob/8629ba46b73f58ac88cacbe8e77c6e9975dd221c/src/font/moderndos/ModernDOS8x14.ttf)
and [ModernDOS8x16.ttf](https://github.com/susam/pcface/blob/8629ba46b73f58ac88cacbe8e77c6e9975dd221c/src/font/moderndos/ModernDOS8x16.ttf).
Both were rasterized without antialiasing at 16 pixels per em, using CP437's
graphical control characters and Unicode mapping. The baselines are 11 and 12
pixels from the top respectively. Character zero is blank. This same process
reproduces the existing 8×8 font byte-for-byte.

Each file stores glyphs in character-code order with one byte per row and the
leftmost pixel in bit 7. These are native font designs, not resized 8×8 glyphs.

| File | Bytes | SHA-256 |
| --- | ---: | --- |
| `modern-dos-8x14.bin` | 3,584 | `be2fa8ee1dbb97fd9d1f0bdee89a19a3c99177dece4b1f098e316dc2e60aed5a` |
| `modern-dos-8x16.bin` | 4,096 | `148582cb5bd61321d485e932b5b0b528e03ec7b4a827cb886a739b18f5180cd8` |
