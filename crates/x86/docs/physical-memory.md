# Physical memory map

`PhysicalMemoryMap` constructs routing metadata independently of execution.
The available execution profiles use virtual memory.

`PhysicalMemoryMap` contains 272 entries for complete 4-KiB physical pages below
0x110000. This covers the highest ordinary real-mode address, 0x10ffef, formed by
`0xffff << 4` plus `0xffff`. The last page includes 16 bytes that canonical
real-mode segment accesses cannot reach. Each page has one kind:

| Mapping | Reads | Writes |
| --- | --- | --- |
| `Ram { backing_offset }` | Backing bytes | Backing bytes |
| `Rom { backing_offset }` | Backing bytes | Ignored |
| `Mmio` | `readMmio` | `writeMmio` |
| `Unmapped` | FF bytes | Ignored |

Unspecified pages are unmapped. Ignoring ROM/hole writes and returning FF from holes
are this backend's machine policy, not universal x86 behavior. Devices such as
flash that interpret writes must use MMIO. Subpage mixtures of RAM, ROM and MMIO
are not represented. Several physical pages may alias the same backing.

`PhysicalMemoryMap::new(regions)` constructs the table from inclusive ranges:

```rust
use wasm86_x86::{PhysicalMapError, PhysicalMapping, PhysicalMemoryMap};

let map = PhysicalMemoryMap::new([
    (0..=0x9ffff, PhysicalMapping::Ram { backing_offset: 0 }),
    (0xf0000..=0xfffff, PhysicalMapping::Rom { backing_offset: 0xa0000 }),
])?;
# Ok::<(), PhysicalMapError>(())
```

Ranges contain complete pages within the table; later regions replace earlier
ones where they overlap. RAM/ROM backing offsets are page aligned and may address
any part of the host's 32-bit backing memory. `get(address)` resolves one physical
byte for host inspection, adjusting direct offsets to that byte.

`to_bytes()` produces a fixed 2176-byte image without a header. Each entry has a
little-endian u32 kind (0 = unmapped, 1 = RAM, 2 = ROM, 3 = MMIO), followed by a u32
backing-page offset. Unmapped and MMIO offsets are zero. Within the image,
physical page `p` has its kind at `p * 8` and backing offset at `p * 8 + 4`.
