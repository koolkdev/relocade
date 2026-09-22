# Host integration

Generated x86 modules use this contract for both snapshot blocks and interpreter
entries. The embedding host supplies memory, dispatch and descriptor resolution.
The Wasm engine must support multiple memories, multiple results, tail calls and
the threads extension's atomic instructions, including on unshared memories.

## Entries and dispatch

`CompiledModule` contains the Wasm bytes, exported entry name and required segment
profile. Snapshot entries are named `block_<hex start_eip>`; interpreter entries
are named `run` or `step`. All have the Wasm signature `() -> i64`.

On success, an entry publishes CPU state, updates EIP and the completed-instruction
count, then tail-calls `wasm86.dispatch(next_eip: i32) -> i64`. Its result is returned
unchanged. EIP and dispatch arguments are offsets relative to CS, and EIP and count
use 32-bit wrapping arithmetic. A taken transfer checks its segment target before
committing instruction effects; fetching the destination belongs to the next entry.

The host chooses whether dispatch enters more code or returns control. Interpreter
`run` continues inside Wasm until a branch or segment load completes. It uses
the same instruction boundary as snapshot compilation: conditional branches end
execution on both outcomes. Earlier instructions remain published if a later
instruction faults or is unsupported. Each instruction fetches live guest bytes
and starts with fresh prefix state. REP completes its repetition and continues
to the next instruction in both frontends.

Interpreter `step` executes one instruction before dispatch, including all elements
of a supported REP instruction. Neither interpreter entry has an execution budget.
Straight-line `run` execution, REP work and chains of host dispatches have no fixed
bound. The snapshot compiler's instruction limit bounds compilation only; it is
not an interpreter limit or a host responsiveness guarantee.

## Imported memories

All imports belong to the `wasm86` module. Memories are distinct, unshared Wasm
memory objects; sizes below are minimum counts of 64-KiB Wasm pages.

| Import | Minimum pages | Contents |
| --- | ---: | --- |
| `cpuState` | 1 | CPU backing image at byte zero. |
| `guest` | 1 | Physical guest RAM. |
| `machine` | 64 | Linear-to-physical page table at byte zero. |

Every interpreter imports all three memories, `dispatch`, `resolveSegment` and
`querySegmentDescriptor`.
Snapshot modules omit imports they do not use: memory instructions require guest
RAM and the page table, segment loads require `resolveSegment`, and LAR/LSL/VERR/VERW
require `querySegmentDescriptor`. Hosts should use the generated module's import
list when instantiating it.

The page table contains 2^20 little-endian u32 entries, one per 4-KiB linear page.
Bit 0 means present, bit 1 permits data writes, and bits 12–31 give the physical
frame address in `guest`; other bits are ignored. Reads and instruction fetches
require presence. Every present frame must have valid physical backing. Violating
that invariant is a host error, not a guest page fault.

Segment translation precedes paging. Linear addresses wrap at 32 bits, and a span
crossing linear zero uses the mappings on both sides. All pages of a data store
are permission-checked before any byte is written, including scattered backing.
Read-modify-write destinations require write permission even when their value
will remain unchanged. Faults identify the first denied byte.

LOCK is supported on eligible memory-destination forms, including CMPXCHG8B.
Naturally aligned operands use native Wasm atomic read-modify-write operations;
memory XCHG does so with or without LOCK. NEG uses a compare-exchange retry loop.
Alignment is determined after segment translation. All operand bytes pass write
checks before the update, including a failed comparison. Ordinary loads, stores
and unlocked updates retain ordinary Wasm accesses; no extra fences are inserted.

Unaligned locked operands use checked reads and writes under the private-memory
contract, which prevents guest interleaving. The current imports do not support
concurrent access to shared guest RAM. Such access will require coordination for
unaligned locked operands as well as the surrounding ordinary accesses.
Rejected LOCK combinations and register forms of memory-only instructions remain
unsupported encodings, reported through the unsupported-instruction path rather
than #UD.

## CPU backing image

Use `CpuState::to_bytes` and `CpuState::from_bytes` to exchange state with CPU memory.
Conversion is explicitly little endian and preserves reserved bytes, inactive flag
payloads, raw segment attributes and x87 encodings. The image is
`CpuState::BYTE_LEN` (312) bytes:

| Byte offset | Contents |
| ---: | --- |
| 0 | 12-byte `StoredStatusSource`: kind, three reserved bytes, two u32 operands. |
| 12 | 12-byte `FlagBytes`: CF, PF, AF, ZF, SF, OF, TF, DF, NT, AC, ID, reserved. |
| 24 | Eight u32 registers: EAX, ECX, EDX, EBX, ESP, EBP, ESI, EDI. |
| 56 | u32 EIP. |
| 60 | Six 12-byte segment records: ES, CS, SS, DS, FS, GS. |
| 132 | Twelve reserved bytes. |
| 144 | u32 completed-instruction count. |
| 148 | Four reserved bytes. |
| 152 | Three u16 x87 fields: control word, full tag word, last opcode. |
| 158 | Two reserved bytes. |
| 160 | Two u32 x87 pointer offsets: instruction, data. |
| 168 | Two u16 x87 pointer selectors: instruction, data. |
| 172 | Eight-byte `StoredX87Status`: exception flags, TOP, C0, C1, C2, C3, ES, B. |
| 180 | Four reserved bytes. |
| 184 | Eight 16-byte physical x87 register slots, R0 through R7. |

`StoredX87Status` stores independent bytes rather than a packed status word.
`exception_flags` uses bits 0–6 for IE, DE, ZE, OE, UE, PE and SF. `top` uses
bits 0–2; `c0`, `c1`, `c2`, `c3`, `error_summary` and `busy` use bit 0.
Serialization preserves the unused bits of these bytes without interpretation.

Each x87 slot contains a u64 significand, a u16 sign/exponent word and six
reserved bytes. The ten value bytes retain the binary80 encoding, including its
explicit integer bit and noncanonical encodings. `status.top` maps
logical ST(i) to these physical registers. The full tag word retains each
register's two-bit tag; marking a register empty does not erase its payload.
`StoredX87` is a host snapshot layout, not an FSAVE or FXSAVE memory operand.

Each segment record contains a u32 base, u32 inclusive byte limit, u16 visible
selector and u16 normalized attributes. Attribute bits 0–4 mean usable, code,
readable-code/writable-data, expand-down and D/B; remaining bits are reserved.
These are loaded caches, not packed GDT/LDT descriptors.

The flag image is storage, not architectural EFLAGS. Status-source kind zero reads
the low bits of the six stored status bytes. Subtraction kinds 1/5/9, addition
kinds 2/6/10 and logic kinds 3/7/11 derive byte/word/dword status flags from the
payload. Arithmetic stores its original zero-extended operands; logic stores its
result in the first operand. The stored status bytes can be stale while a source
is active. Control and system flags always use their stored bytes. Valid source
kinds are an internal invariant; a host setting concrete status flags must also
select kind zero.

Architectural flag transfers use a fixed CPL3/IOPL0 model with IF set and
VM/RF/VIF/VIP clear. Word stack images restore the represented low flags;
dword images also restore AC and ID. IRETD restores RF on hardware, but RF is
unrepresented here along with debug delivery. TF and AC are stored without
enabling debug traps or alignment checks.

```rust
use wasm86_x86::CpuState;

let mut cpu = CpuState::default();
cpu.registers.eax = 42;
cpu.eip = 0x1000;
let image = cpu.to_bytes(); // Copy to byte zero of the imported CPU memory.
assert_eq!(CpuState::from_bytes(image), cpu);
```

`CpuState::default()` installs flat segment caches with zero visible selectors
and initializes the x87 control word to `037F`, status to zero and tags to `FFFF`.
Other fields are zero. It is a host execution configuration, not a processor
reset or a segment-load operation. `filled` and `from_bytes` preserve literal
images; an all-zero image has unusable segment caches. Hosts using far CALL/RET
must initialize CS with a valid return selector and provide its descriptor.

## x87 environment and stack

The implemented controls are FNINIT, FNCLEX, FLDCW, FNSTCW, FNSTSW (memory and AX)
and standalone FWAIT. Stack operations include FLD ST(i), FST/FSTP ST(i), FXCH,
FFREE, FINCSTP and FDECSTP. Memory data transfers support FLD m80 and FSTP m80;
there is no FST m80 encoding. Arithmetic and binary32/binary64 transfers remain
unsupported.
Execution assumes an enabled FPU with native exception reporting, corresponding
to CR0.EM=0, CR0.TS=0 and CR0.NE=1. CR0 and device-not-available exceptions are not
modeled by this user-mode environment.

FWAIT and FLDCW observe `status.error_summary`, the architectural ES bit. A pending
exception returns #MF at that waiting instruction's EIP without retirement.
The stored x87 instruction and data pointers retain the previous operation;
they are not replaced by the waiter's address. Hosts supplying x87 state must
keep ES/B consistent with its sticky exception flags and control masks.

FLDCW first checks for a pending exception, then reads its two-byte operand.
A successful load updates the control word and refreshes ES/B from the retained
exception flags and new masks. Newly unmasked exceptions become pending; the
load itself completes and the next waiter reports #MF. The pending check before
a simultaneously faulting operand is this implementation's ordering policy;
Intel does not specify a universal priority for those competing execution faults.

The no-wait controls bypass pending-exception delivery. FNINIT resets the x87
environment while preserving raw register payloads and snapshot padding.
FNCLEX clears exception flags, stack fault, ES and B; this implementation retains
the otherwise undefined condition codes. Control/status stores use two bytes
regardless of the operand-size prefix. FNSTSW AX preserves EAX's upper half.
The control instructions otherwise leave the x87 pointers and opcode unchanged.
Reserved control bits have no execution meaning; their readback is not a
Pentium 4 compatibility guarantee.

Waiting spellings such as FINIT and FSTCW encode a separate `9B` FWAIT followed
by the no-wait instruction. They retain separate retirement and fault boundaries.
The behavior above follows Intel's Pentium-4-era manuals:
[Volume 1](https://kib.kiev.ua/x86docs/Intel/SDMs/253665-014.pdf), sections
8.1.7–8.1.8 and 8.3.12, and the control-instruction entries in
[Volume 2A](https://kib.kiev.ua/x86docs/Intel/SDMs/253666-014.pdf).

Extended transfers preserve all ten value bytes, including signaling NaNs,
pseudo-denormals and unsupported encodings; these movements do not perform
arithmetic or rounding. Nonempty destination tags describe the stored encoding.
Physical-slot padding stays with its storage location.
FLD ST(i) reads its source relative to the old TOP, before pushing. FSTP ST(i)
writes its destination before marking the old stack top empty and advancing TOP.
FFREE changes only its register tag; rotations change TOP without moving payloads.

Stack faults set IE and SF. C1 distinguishes overflow from underflow; a source
underflow takes priority over a destination overflow. Masked faults substitute
the extended indefinite value and perform the instruction's push or pop.
For FXCH, each empty source is replaced before the two values are exchanged.
An unmasked stack fault suppresses data and stack changes, records pending
status, and retires its producer. Integer and no-wait instructions can continue;
the next waiting instruction reports #MF. Reporting it does not clear the status.

The complete ten-byte memory span passes segment and page checks before a
transfer changes data, stack state or numerical pointers. This implementation
checks these accesses before generating a new stack fault. A preexisting pending
exception is checked first. Faulting stores do not write an earlier portion of
the value or pop the stack.

Stack and data-transfer instructions record their instruction offset, selector
and opcode. Memory forms also record their effective offset and segment selector;
register forms preserve the otherwise undefined data pointer. Opcode recording
uses the Pentium 4 compatibility-mode policy, keeping the last x87 opcode valid
after every such instruction. Operand-size prefixes do not change the width of
these transfers; address-size and segment prefixes retain their ordinary meaning.

Status fields remain separate in generated execution and in the host snapshot.
FNSTSW assembles the architectural status word when requested; exits publish
changed fields directly. Untouched imported fields retain every byte, including
unused bits and ES/B. Changing a field may normalize that field's unused bits.

## Entry validity

The host must admit each entry against `CompiledModule::segment_profile` before
executing it, including when following a direct dispatch link.

| Profile | Compilation assumptions |
| --- | --- |
| `Flat32` | Usable, flat readable CS and writable expand-up DS/ES/SS; zero bases, full u32 limits, CS.D=1 and SS.B=1. FS/GS are runtime inputs. |
| `Segmented32` | CS.D=1; segment access and SS.B are checked or read at runtime. |
| `Segmented16` | CS.D=0; segment access and SS.B are checked or read at runtime. |

`SegmentProfile::is_compatible_with` tests those assumptions. Selectors, reserved
attribute bits and ordinary DS/ES D/B bits do not determine flat compatibility.
Segmented profiles can admit unusable caches or restrictive limits so execution
can report their guest faults. They describe protected-mode defaults, not real mode.

Flat entries omit checks and base reads where the segment is known to satisfy the
profile. Interpreter memory operands with an explicit segment override use checked
translation; size and REP prefixes alone preserve the default-segment shortcut.
The selected profile must remain valid until a terminal segment load commits its
cache. Only state publication and dispatch follow that commit; the next entry
must be admitted against the new state.

Snapshot blocks have an additional requirement: every compiled instruction's full
fetch span must fit executable CS, and its bytes must match readable guest memory
at CS.base + its EIP, with 32-bit linear wrapping. The byte-only compiler cannot
establish this, and generated blocks do not repeat instruction-fetch checks.
Bytes after the compilation boundary are irrelevant. Profile compatibility alone
does not establish snapshot validity.

The host must preserve validity during execution and revalidate or invalidate
affected entries and links when relevant CS state, code bytes or mappings change.
An instruction changing a relied-upon assumption must end the block. There is no
code cache or automatic invalidation in these libraries. A checked snapshot
producer stops before an invalid fetch, executes any valid instruction prefix,
then handles the fault, for example by entering the interpreter at the failing EIP.

The interpreter checks required instruction bytes through CS and paging at runtime.
Its direct decoding loop may retain the current page-table entry until it leaves
that decoder invocation. It still fetches live guest bytes and checks every access;
no mapping cache survives a host dispatch or a new interpreter entry. The host may
therefore remap code pages between entries under the existing validity rules.
CS checks precede page checks for each byte. It fetches all fields of a supported
form before data access, but rejects an unsupported group extension before reading
unneeded fields. Requiring byte sixteen raises #GP(0); an earlier unavailable
required byte faults first. It never fetches a branch destination as part of the
transferring instruction.

## Segment resolver

Segment loads call `wasm86.resolveSegment(segment: i32, selector: i32)`, returning
six Wasm i32 results: `(status, error_code, base, limit, selector, attributes)`.
Segment indices are ES=0, CS=1, SS=2, DS=3, FS=4 and GS=5. Selector inputs and
selector/attribute outputs use zero-extended 16-bit values.

Status zero supplies a complete normalized `StoredSegment`. Failure statuses are
architectural vectors 11 (#NP), 12 (#SS) or 13 (#GP); only their error code is used.
Unknown statuses or invalid success records violate the host contract. Wasm owns
the instruction's remaining checks, cache commitment, retirement and fault exit.
In particular, resolving CS does not validate a transfer's target offset.

RETF and IRET require a return selector with RPL 3 before resolving CS. IRET
checks all three slots and reads their values before resolving the selector.
It commits CS, EIP, flags and the stack pointer only after all checks succeed.
An entry NT flag of one requests an unsupported task return before ordinary stack
access. Restoring NT from the frame affects a later IRET, not the current return.

The callback resolves the current thread's descriptor view, for example through
`DescriptorTables::resolve_user_segment`. It must not inspect or mutate CPU state,
guest RAM or page tables, or reenter guest execution. Descriptor-table edits affect
future loads and leave already-loaded CPU caches unchanged.

`DescriptorTables` provides host-managed global/local slots keyed by selector table
and index bits, ignoring RPL for lookup. Resolution applies protected-mode CPL3
load rules; it neither allocates Windows selectors nor models guest GDTR/LDTR,
packed descriptor memory or descriptor accessed-bit writes. All represented
descriptors have A=1 and L=0. `SegmentLimit` keeps the encoded 20-bit limit and
granularity together; loaded caches still use an effective byte limit. See the
type's API documentation for load validation and descriptor construction.

## Descriptor queries

LAR/LSL/VERR/VERW call `wasm86.querySegmentDescriptor(selector: i32)` after reading
their 16-bit selector operand. The input is zero-extended. The callback returns
five Wasm i32 results in this order: `(visible, readable, writable, accessRights,
limit)`. The first three results are booleans represented as 0 or 1. The results
describe code/data descriptors at CPL3:

| Result | Meaning |
| --- | --- |
| `visible` | Visible; LAR and LSL use this as ZF. |
| `readable` | Readable; VERR uses this as ZF. |
| `writable` | Writable; VERW uses this as ZF. |
| `accessRights` | LAR's 32-bit result, with the bit positions specified below. |
| `limit` | Inclusive effective byte limit, with page granularity already expanded. |

`accessRights` uses bit positions within the descriptor's upper 32 bits: type 11:8,
S 12, DPL 14:13, P 15, AVL 20, L 21, D/B 22 and G 23. All remaining bits are zero
in this model, including the architecturally undefined bits 19:16.

Null, missing and privilege-inaccessible descriptors return all zeros. Presence
affects the reported P bit, not visibility or read/write permission. Execute-only
code can be visible without either permission; code is
never writable. LAR/LSL preserve the destination on failure and truncate successful
results to the operand width. All four instructions change only ZF among flags.

The represented environment contains only code/data entries, which share LAR/LSL
eligibility. System descriptors and gates are outside this contract. In particular,
a future gate model needs separate LAR/LSL eligibility. A=1 and L=0 match the host
descriptor invariants; undefined LAR bits 19:16 are chosen as zero.

The callback queries the current thread's descriptor table, for example through
`DescriptorTables::query_user_segment_descriptor`, which returns a
`SegmentDescriptorInfo` with named booleans and values. It must not inspect or
mutate CPU state, guest RAM or page tables, or reenter guest execution. It does not
load a segment or fault for a rejected selector. Table edits affect the next query
while loaded caches remain unchanged. Guest operand reads can still fault before
the callback runs.

## Fault and unsupported exits

A guest fault returns directly without calling dispatch. Earlier completed
instructions remain published; EIP identifies the faulting instruction, which
does not retire. Its entry CPU state is preserved except for the partial progress
described below.

Each checked guest-memory write is complete or absent. Instructions with ordered
multiple writes, such as ENTER and PUSHA, retain completed writes if a later access
faults. REP faults retain completed elements, current indices and the remaining
count, with EIP at the instruction's first prefix. Repeated CMPS/SCAS faults retain
the flags from instruction entry. A successful repetition keeps the last comparison's
flags; zero-count execution preserves them. A successful REP retires once.

POPA also retains registers restored before a later fault. ESP and EIP remain at
instruction entry and the instruction does not retire. Every slot is checked
against SS and paging in order, including the discarded SP/ESP slot, whose value
is not read. This models observed partial progress; Intel documents incomplete
register restoration on POPAD faults without specifying every CPU's exact pattern.

The u64 return format is `(tag << 48) | (payload << 32) | address`, with a 16-bit
tag, 16-bit payload and 32-bit address. These tags are independent of architectural
exception vector numbers:

| Exit | Tag | Payload | Address |
| --- | ---: | --- | --- |
| Divide error | 1 | Zero | Zero |
| General protection | 2 | Error code | Zero |
| Page fault | 4 | Error code | First denied linear byte |
| Unsupported instruction | 8 | Diagnostic opcode byte | Instruction's starting EIP |
| Stack fault | 16 | Error code | Zero |
| Segment not present | 32 | Error code | Zero |
| BOUND range exceeded | 64 | Zero | Zero |
| Invalid opcode | 128 | Zero | Zero |
| Floating-point error | 256 | Zero | Zero |

Floating-point error reports #MF (architectural vector 16). Its return value
contains no payload; the published x87 status and environment describe the
pending exception.

UD2 raises invalid opcode (#UD, architectural vector 6) after its complete encoding
has been fetched. It preserves CPU state and memory, does not retire or dispatch,
and leaves EIP at its first prefix or opcode byte. Snapshot compilation ends at
UD2 without decoding subsequent bytes.

Page-fault error bit 0 means a present but denied page, bit 1 means a data write,
and bit 4 means instruction fetch. Cached segment-access failures use #SS(0) for
SS and #GP(0) for other segments. Resolver faults retain their selector error code.

An unsupported exit describes an encoding or execution path outside the
implementation's subset, not an architectural invalid-opcode exception. Its
diagnostic byte is the first byte after size and segment prefixes, `0F` for an
extended opcode, or the selected `F0`/`F2`/`F3` prefix for an unsupported prefixed
form. It does not retire or dispatch. IRET with entry NT set reports `CF` at the
instruction's restart EIP, including its prefixes. Snapshot construction reports
`BlockError` for unsupported encodings; state-dependent unsupported paths remain
runtime exits. Truncated byte input alone cannot establish a guest fetch fault.

Faults are reported to the host. Guest IDT delivery, privilege transitions,
interrupt/debug delivery, real-mode transfers and SS-load inhibition are not
modeled. Native Wasm traps from broken backing or internal arithmetic invariants
are implementation errors, outside this guest-fault protocol.
