# Physical memory

`ExecutionProfile::Real16` translates segment offsets to physical addresses, then
uses generated Wasm to route accesses. RAM and ROM reads use `guest` backing;
only MMIO accesses call the host. Ordinary real mode does not raise a page fault
for an absent physical mapping. A20 remains enabled in this profile.

## Physical page map

The host supplies two distinct, unshared Wasm memories in the `wasm86` module:
`guest` for backing bytes and `physicalMap` for routing metadata. Both imports
have a minimum size of one 64-KiB Wasm page. The host allocates enough memory for
all mapped backing. The routing table fits in one Wasm page.

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
backing-page offset. Unmapped and MMIO offsets are zero. Install the image at
offset zero in `physicalMap`; physical page `p` has its kind at `p * 8` and backing
offset at `p * 8 + 4`. Runtime mapping changes update the installed Wasm table.

## MMIO callbacks

Both functions belong to the `wasm86` import module:

| Import | Wasm signature |
| --- | --- |
| `readMmio` | `(address: i32, bytes: i32) -> i64` |
| `writeMmio` | `(address: i32, bytes: i32, value: i64) -> ()` |

Addresses use an unsigned i32 carrier within the real-mode physical range.
`bytes` is a count from 1 through 8; the whole request lies in MMIO.
Values use little-endian order in the low `bytes * 8` bits. Upper read-result bits
are ignored; upper write-value bits are zero. The i64 is a value container, not an
eight-byte device access.

Each transferred operand field retains its width when its whole span is MMIO,
including across adjacent MMIO pages. A span crossing different routing kinds
is split at the page boundary. A four-byte field can therefore leave a three-byte
MMIO portion. The host adapter applies its bus and device rules to each request;
one callback need not correspond to one hardware bus transaction. Wider structured
operands transfer their constituent fields separately.

Callbacks complete synchronously and execute even when a read result is unused.
They may change backing bytes and installed routing. The next transfer, including
the remaining portion of a split access, reads the current routing. An already
issued MMIO request covers its entire selected span. Callbacks must not inspect or
modify CPU backing state or reenter guest execution. There is no retry result;
unexpected host or Wasm traps are implementation errors.

Execution assumes one guest CPU with private backing. The host must not advance
other CPUs or bus masters during an entry. Locked updates use a read followed by
a write under this contract; the ABI does not expose a lock boundary for
concurrent DMA or another processor.

## Generated access and fetch

Segment checks establish physical table bounds before lookup, including for
speculative fetch windows. A small inline check handles direct accesses within
one RAM/ROM page. Routing,
splitting and partial transfer assembly share one reader and one writer per
module, created only when used. Helpers are Wasm functions; host calls occur only
in their MMIO branches. Separate snapshot modules each contain their own helpers.

The interpreter checks CS and probes RAM/ROM for a direct fetch window without
calling a device. Windows stop at page boundaries; unavailable windows use exact
reads of required bytes. Actual MMIO instruction fetches use one-byte requests
in decoder order, without reading ahead. This is the emulator's functional fetch
contract, not a model of a particular CPU's prefetch bus transactions. Physical
routing is rechecked after each instruction and is not held in the protected
interpreter's page cache.

Snapshot blocks use compiled bytes under the ordinary host validity contract.
Code and mapping changes must preserve that contract throughout execution; the
interpreter can observe code remapping at the next instruction.
