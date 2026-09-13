# wasm86

Rust components for x86 execution in WebAssembly.

`wasm86-x86` compiles MOV, MOVZX, MOVSX, CBW, CWDE, CWD, CDQ, LEA, XCHG, XADD, CMPXCHG, CMOVcc, ADD, ADC,
SUB, SBB, CMP, AND, OR, XOR, TEST, INC, DEC, NEG, NOT, MUL, IMUL, DIV, IDIV, SHL, SHR, SAR, SHLD, SHRD,
ROL, ROR, RCL, RCR, BT, BTS, BTR, BTC, BSF, BSR, PUSH, POP, PUSHF/PUSHFD, POPF/POPFD,
SETcc, CALL, RET, JMP, Jcc, JECXZ, LOOP, LOOPE, LOOPNE, CLC, STC, CMC, CLD, STD, LAHF, SAHF,
MOVS, STOS, LODS, CMPS and SCAS blocks, including REP MOVS/STOS,
from byte snapshots:

```rust
let block = wasm86_x86::compile_block_from_bytes(0x1000, &[0xb8, 42, 0, 0, 0], 1)?;
```

Compilation stops at the first branch, REP or the requested instruction limit,
whichever comes first. A conditional branch ends the block whether taken or not.
Missing, overlong or unsupported instructions before that boundary are construction
errors; bytes after it are ignored. The returned
module exports `block_1000` and imports `wasm86.cpuState` memory (minimum one
64-KiB page) and `wasm86.dispatch(i32) -> i64`.

`CpuState` exposes the backing state as plain Rust fields. `Registers`,
`StoredFlags`, `StoredStatusSource` and `FlagBytes` retain every field, including
reserved bytes and inactive flag data. The types support copying and full equality comparisons:

```rust
let mut cpu = wasm86_x86::CpuState::default();
cpu.registers.ebx = 0x1234_5678;
cpu.flags.bytes.df = 1;
cpu.eip = 0x1000;
let bytes = cpu.to_bytes();
assert_eq!(wasm86_x86::CpuState::from_bytes(bytes), cpu);
```

Copy `cpu.to_bytes()` into the start of the imported CPU memory to initialize it.
Decode its first `CpuState::BYTE_LEN` bytes with `CpuState::from_bytes` to inspect
the result. Conversion is explicitly little endian on every host and preserves
noncanonical flag bytes without interpreting them. The backing image is 152 bytes:
EAX through EDI are dwords in encoding order at offsets 24–52, EIP is at 56, and the
completed-instruction count is at 144. `Registers` also supports indexing by
`Gpr32` for cases that select a register dynamically.

The flag backing record separates the source of status values from the individual
stored flag bytes:

```rust,ignore
struct StoredFlags {
    status_source: StoredStatusSource, // kind, reserved bytes and source payload
    bytes: FlagBytes,                  // cf, pf, af, zf, sf, of, tf, df, nt, ac, id, reserved
}
```

`status_source` occupies bytes 0–11. Kind zero uses the low bits of the stored
CF/PF/AF/ZF/SF/OF bytes; other supported kinds derive those six status values from
arithmetic operands or a logical result. Their stored bytes can therefore be stale. `bytes` occupies
bytes 12–23: the six status flags, then TF, DF, NT, AC, ID and a reserved byte.
DF is x86's control flag; TF, NT, AC and ID belong to the system flags. Their values
always use their stored bytes, independently of `status_source`.
This is wasm86's backing format, not the architectural EFLAGS bit encoding.
Host field access inspects raw storage; generated `read_flag` operations resolve
the current logical values. The logical `StatusFlag` subset describes the flags
that arithmetic can produce without dividing the stored bytes into separate groups.

An exit publishes its current status source and direct flag definitions, then
dirty register definitions in first-write order.
EIP and count use 32-bit wrapping arithmetic.
The number of completed instructions is fixed during compilation for each exit.
An exit with progress reads the runtime counter and adds that number; an exit
with no progress leaves it unchanged. The block
tail-calls dispatch with the next EIP and returns its result. This snapshot path
supports these forms in default-32 operand and address mode:

| Operands | Byte | Word (`66`) | Dword |
| --- | --- | --- | --- |
| MOV opcode-selected register and immediate | B0–B7 | B8–BF | B8–BF |
| MOV register and register/memory | 88/8A | 89/8B | 89/8B |
| MOV register/memory destination and immediate | C6 /0 | C7 /0 | C7 /0 |
| MOV accumulator and absolute memory offset | A0/A2 | A1/A3 | A1/A3 |
| CBW/CWDE sign extension within the accumulator | — | 98 | 98 |
| CWD/CDQ sign extension into the high accumulator | — | 99 | 99 |
| ADD register and register/memory | 00/02 | 01/03 | 01/03 |
| ADD accumulator and immediate | 04 | 05 | 05 |
| ADD register/memory and immediate | 80 /0 | 81/83 /0 | 81/83 /0 |
| ADC register and register/memory | 10/12 | 11/13 | 11/13 |
| ADC accumulator and immediate | 14 | 15 | 15 |
| ADC register/memory and immediate | 80 /2 | 81/83 /2 | 81/83 /2 |
| SUB register and register/memory | 28/2A | 29/2B | 29/2B |
| SUB accumulator and immediate | 2C | 2D | 2D |
| SUB register/memory and immediate | 80 /5 | 81/83 /5 | 81/83 /5 |
| SBB register and register/memory | 18/1A | 19/1B | 19/1B |
| SBB accumulator and immediate | 1C | 1D | 1D |
| SBB register/memory and immediate | 80 /3 | 81/83 /3 | 81/83 /3 |
| CMP register and register/memory | 38/3A | 39/3B | 39/3B |
| CMP accumulator and immediate | 3C | 3D | 3D |
| CMP register/memory and immediate | 80 /7 | 81/83 /7 | 81/83 /7 |
| AND register and register/memory | 20/22 | 21/23 | 21/23 |
| AND accumulator and immediate | 24 | 25 | 25 |
| AND register/memory and immediate | 80 /4 | 81/83 /4 | 81/83 /4 |
| OR register and register/memory | 08/0A | 09/0B | 09/0B |
| OR accumulator and immediate | 0C | 0D | 0D |
| OR register/memory and immediate | 80 /1 | 81/83 /1 | 81/83 /1 |
| XOR register and register/memory | 30/32 | 31/33 | 31/33 |
| XOR accumulator and immediate | 34 | 35 | 35 |
| XOR register/memory and immediate | 80 /6 | 81/83 /6 | 81/83 /6 |
| TEST register/memory and register | 84 | 85 | 85 |
| TEST accumulator and immediate | A8 | A9 | A9 |
| TEST register/memory and immediate | F6 /0 | F7 /0 | F7 /0 |
| INC opcode-selected register | — | 40–47 | 40–47 |
| DEC opcode-selected register | — | 48–4F | 48–4F |
| INC register/memory | FE /0 | FF /0 | FF /0 |
| DEC register/memory | FE /1 | FF /1 | FF /1 |
| NOT register/memory | F6 /2 | F7 /2 | F7 /2 |
| NEG register/memory | F6 /3 | F7 /3 | F7 /3 |
| MUL implicit accumulator and register/memory | F6 /4 | F7 /4 | F7 /4 |
| IMUL implicit accumulator and register/memory | F6 /5 | F7 /5 | F7 /5 |
| IMUL register destination and register/memory | — | 0F AF | 0F AF |
| IMUL register destination, register/memory and immediate | — | 69/6B | 69/6B |
| DIV implicit dividend and register/memory divisor | F6 /6 | F7 /6 | F7 /6 |
| IDIV implicit dividend and register/memory divisor | F6 /7 | F7 /7 | F7 /7 |
| ROL register/memory by one, CL or imm8 | D0/D2/C0 /0 | D1/D3/C1 /0 | D1/D3/C1 /0 |
| ROR register/memory by one, CL or imm8 | D0/D2/C0 /1 | D1/D3/C1 /1 | D1/D3/C1 /1 |
| RCL register/memory by one, CL or imm8 | D0/D2/C0 /2 | D1/D3/C1 /2 | D1/D3/C1 /2 |
| RCR register/memory by one, CL or imm8 | D0/D2/C0 /3 | D1/D3/C1 /3 | D1/D3/C1 /3 |
| SHL/SAL register/memory by one, CL or imm8 | D0/D2/C0 /4 | D1/D3/C1 /4 | D1/D3/C1 /4 |
| SHR register/memory by one, CL or imm8 | D0/D2/C0 /5 | D1/D3/C1 /5 | D1/D3/C1 /5 |
| SAR register/memory by one, CL or imm8 | D0/D2/C0 /7 | D1/D3/C1 /7 | D1/D3/C1 /7 |
| SHLD register/memory and register by imm8 or CL | — | 0F A4/A5 | 0F A4/A5 |
| SHRD register/memory and register by imm8 or CL | — | 0F AC/AD | 0F AC/AD |
| BT register/memory, register or imm8 | — | 0F A3; 0F BA /4 | 0F A3; 0F BA /4 |
| BTS register/memory, register or imm8 | — | 0F AB; 0F BA /5 | 0F AB; 0F BA /5 |
| BTR register/memory, register or imm8 | — | 0F B3; 0F BA /6 | 0F B3; 0F BA /6 |
| BTC register/memory, register or imm8 | — | 0F BB; 0F BA /7 | 0F BB; 0F BA /7 |
| BSF register, register/memory | — | 0F BC | 0F BC |
| BSR register, register/memory | — | 0F BD | 0F BD |
| PUSH opcode-selected register | — | 50–57 | 50–57 |
| POP opcode-selected register | — | 58–5F | 58–5F |
| PUSH immediate | — | 68/6A | 68/6A |
| PUSH register/memory | — | FF /6 | FF /6 |
| POP register/memory | — | 8F /0 | 8F /0 |
| PUSHF/PUSHFD architectural flag image | — | 9C | 9C |
| POPF/POPFD architectural flag image | — | 9D | 9D |
| MOVS memory at ESI to memory at EDI | A4 | A5 | A5 |
| CMPS memory at ESI minus memory at EDI | A6 | A7 | A7 |
| STOS accumulator to memory at EDI | AA | AB | AB |
| LODS memory at ESI to accumulator | AC | AD | AD |
| SCAS accumulator minus memory at EDI | AE | AF | AF |
| CALL relative | — | E8 | E8 |
| CALL register/memory | — | FF /2 | FF /2 |
| RET with optional imm16 cleanup | — | C3/C2 | C3/C2 |
| JMP register/memory | — | FF /4 | FF /4 |
| JECXZ relative byte displacement | — | E3 | E3 |
| LOOP relative byte displacement | — | E2 | E2 |
| LOOPE/LOOPZ relative byte displacement | — | E1 | E1 |
| LOOPNE/LOOPNZ relative byte displacement | — | E0 | E0 |
| SETcc register/memory destination | 0F 90–9F | — | — |
| XCHG register/memory and register | 86 | 87 | 87 |
| XCHG accumulator and opcode-selected register | — | 90–97 | 90–97 |
| XADD register/memory destination and register | 0F C0 | 0F C1 | 0F C1 |
| CMPXCHG register/memory destination and register | 0F B0 | 0F B1 | 0F B1 |

CLC (`F8`) clears CF, STC (`F9`) sets CF, and CMC (`F5`) complements CF.
They preserve the other five logical status flags. CLD (`FC`) clears DF and
STD (`FD`) sets DF, preserving the complete status record. These five instructions
have no operands, ignore `66`, and preserve every other register and flag.
LAHF (`9F`) packs the logical SF/ZF/AF/PF/CF values into AH bits 7/6/4/2/0;
bit 1 is set and bits 3 and 5 are clear. It preserves the complete flag record,
including a stored or pending arithmetic source. SAHF (`9E`) copies those five
bits from AH into the status flags and ignores AH bits 1, 3 and 5. It preserves
OF, DF and the other flags. Both instructions use AH with or without `66`,
preserve AL and EAX's upper half, and need no data-memory access. Their byte
image follows the [Intel instruction reference](https://cdrdv2-public.intel.com/868137/325462-089-sdm-vol-1-2abcd-3abcd-4.pdf);
it is independent of the raw `FlagBytes` backing layout.
Their effects follow the corresponding entries in the
[Intel Software Developer's Manual](https://www.intel.com/content/www/us/en/developer/articles/technical/intel-sdm.html).

MOVZX (`0F B6`/`0F B7`) and MOVSX (`0F BE`/`0F BF`) read a byte or word
register/memory source into a register destination. MOVZX fills the added bits
with zero; MOVSX repeats the source sign bit. The destination is a dword, or a
word with `66`; the source width remains fixed by the opcode. Word destinations
preserve the register's upper half, and all forms preserve flags. Only the source
span is read and checked, even when the destination is wider. The `66 0F B7` and
`66 0F BF` word-to-word forms copy the source unchanged. These forms are accepted
by [Intel XED](https://github.com/intelxed/xed/blob/main/datafiles/xed-isa.txt),
although the SDM's ordinary MOVZX/MOVSX opcode tables omit them.

CBW (`66 98`) sign-extends AL into AX; CWDE (`98`) sign-extends AX into EAX.
CWD (`66 99`) fills DX with the sign of AX; CDQ (`99`) fills EDX with the sign
of EAX. CWD and CDQ preserve the entire input accumulator. CBW preserves EAX's
upper half, and CWD preserves EDX's upper half. All four preserve every flag.
Their encodings contain only the opcode and any operand-size prefix. These
rules follow the CBW/CWDE and CWD/CDQ entries in the
[Intel instruction reference](https://cdrdv2-public.intel.com/868137/325462-089-sdm-vol-1-2abcd-3abcd-4.pdf).

XCHG exchanges two old values and preserves flags. `86` exchanges a byte and `87`
exchanges a dword, or a word with `66`. `90`–`97` exchange EAX with an opcode-selected
register, or AX with `66`; `90` and `66 90` are NOP aliases. Byte and word exchanges
preserve the other register bits. Memory addresses use the original base and index
values, including when either register is exchanged. The full memory span must be
writable before either operand changes, even when their old values are equal.
Memory XCHG is implicitly locked on x86. wasm86 uses unshared WebAssembly memories
and completes the exchange before returning to the host; concurrent shared-memory
execution is outside this ABI.

XADD adds the two old operands into the destination, copies the old destination
to the source register, and sets all six flags like ADD. When both operands name
the same register, that register receives the sum. CMPXCHG compares AL, AX or EAX
with the destination and sets all six flags like accumulator minus destination.
On equality it copies the source register to the destination; otherwise it copies
the old destination to the accumulator. Byte and word writes preserve the other
register bits. Both instructions resolve memory addresses from the old registers
and require full write permission before changing registers, memory or flags;
CMPXCHG requires it even when the comparison fails. CMPXCHG8B and explicit LOCK
prefixes remain outside the supported subset.

MUL multiplies unsigned operands; IMUL multiplies signed operands. Their implicit
forms multiply AL, AX or EAX by a same-width register or memory source and write
the full product to AX, DX:AX or EDX:EAX. A byte operation writes the whole AX;
word operations preserve the upper halves of EAX and EDX. Two-operand IMUL (`0F AF`)
multiplies the old destination by its source. Three-operand IMUL (`69`/`6B`)
multiplies the source by an immediate. These explicit forms write only the low
word or dword. `69` encodes an operand-sized
immediate; `6B` sign-extends its immediate byte to that width.

MUL sets CF and OF when the high half of the product is nonzero. IMUL sets both
when sign-extending the low half would not reproduce the full signed product.
Otherwise both flags are clear. Overflow does not fault. PF/AF/ZF/SF are undefined;
wasm86 chooses 1/0/0/0. Every source and its address use the old register values.
Memory sources need only read permission, and a source fault leaves all results
and flags unchanged. These rules follow the MUL and IMUL entries in the
[Intel instruction reference](https://cdrdv2-public.intel.com/868137/325462-089-sdm-vol-1-2abcd-3abcd-4.pdf).

DIV divides unsigned operands; IDIV divides signed operands. The dividend is all
of AX, DX:AX or EDX:EAX, and the divisor is a byte, word or dword register/memory
source. Quotient and remainder replace AL/AH, AX/DX or EAX/EDX. Byte and word
results preserve the other parent-register bits. IDIV truncates toward zero and
gives a nonzero remainder the dividend's sign. Zero divisors and quotients outside
the destination's unsigned or signed range raise divide error before either
result changes. All six status flags are undefined on success; wasm86 preserves
their incoming record as its deterministic policy. Original registers supply
the dividend, divisor and effective address, including overlapping registers.
The source needs only read permission and is checked before arithmetic errors.
These arithmetic and divide-error rules follow the DIV/IDIV entries and Vol. 3A
section on #DE in the
[Intel manual](https://cdrdv2-public.intel.com/868137/325462-089-sdm-vol-1-2abcd-3abcd-4.pdf).

SHL (also named SAL), SHR and SAR take a count of one, CL or an immediate byte.
All counts are masked to five bits, including byte and word operands. A zero
masked count preserves the value and flags. Memory still requires write permission
for the entire operand before the instruction can complete. CL and the destination
are read before writing, including when the destination is CL, CH, CX or ECX.
SHR fills vacated bits with zero; SAR repeats the original sign bit.
For nonzero counts, PF/ZF/SF describe the result. CF holds the last shifted-out bit,
except that SHL/SHR leave CF undefined at counts at or above the operand width.
OF is defined only for count one: SHL uses the result sign XOR CF, SHR uses the
original sign, and SAR clears it. AF is undefined for every nonzero count.
wasm86 chooses zero for those undefined CF/OF/AF values.

SHLD and SHRD shift a word or dword destination while filling from a same-width
source register. They take CL or an immediate byte as their third operand, with
the same five-bit count mask. The source and old CL are captured before any
destination write, including when they alias the destination. A zero masked
count preserves the destination and entire flag source; memory still requires
write permission for the complete span. A word count of sixteen copies the
source into the destination, with CF holding the last bit removed from the old
destination. For counts from one through the operand width, PF/ZF/SF describe
the result; OF at count one is the old sign XOR the new sign. AF is undefined,
and OF is undefined above one; wasm86 chooses zero for both. For word counts
17–31, the architecture leaves the result and all six flags undefined. wasm86
chooses a zero result, clears CF/AF/OF, and computes PF/ZF/SF from that result.

ROL and ROR rotate within the operand width. They use the same count forms, mask,
old-CL capture and full-span write checks as shifts. A zero masked count preserves
the entire flag source. For nonzero masked counts, ROL copies result bit zero to
CF; ROR copies the result sign bit. PF/AF/ZF/SF retain their prior logical values.
A full byte or word turn can therefore leave the operand unchanged while changing
CF. OF is defined only when the masked count is one: ROL uses result sign XOR CF,
and ROR uses the XOR of the top two result bits. wasm86 chooses zero for undefined
OF, including a byte rotate by nine.

RCL and RCR rotate through the incoming CF, which adds one bit to the ring.
After masking to five bits, counts wrap modulo 9 for bytes and 17 for words;
dword counts remain 0–31 in a 33-bit ring. A zero effective count, including a
complete carry-ring turn, preserves the operand and entire flag source.
For nonzero effective counts, OF uses the same left/right rules as ROL/ROR only
when the masked count is one; wasm86 chooses zero for larger masked counts,
including a byte RCL/RCR by ten. PF/AF/ZF/SF, old-CL capture and full-span memory
write checks follow ROL/ROR. The undocumented group `/6` SHL alias is outside
this subset.

BT copies a bit from a word/dword operand into CF. BTS, BTR and BTC also set,
reset or complement that bit, respectively. CF always receives the old bit.
ZF is unchanged; OF/SF/AF/PF are undefined, and wasm86 preserves their prior
logical values. A register destination masks either kind of offset modulo its
width. For memory, an immediate byte also selects only within the addressed
word/dword: high immediate bits never advance the address.

A memory operand with a register offset instead addresses a bit string. The
offset is signed at the operand width; dividing it by that width, rounding
toward negative infinity, selects the word/dword unit. With DX equal to `FFFF`,
`BT word [EBX],DX` reads bit 15 of the word at EBX minus two.
The original base need not be aligned. The unit's byte offset is added with
32-bit wrapping, then the complete selected unit follows the usual memory
range and page checks. BT needs only read access. BTS/BTR/BTC require full write
permission even when the bit already has the requested value. Offset and address
registers use their old values, and a fault preserves the operand and all flags.

BSF and BSR write the position of the lowest or highest set bit, respectively,
from a word/dword source into a same-width register. Positions start at zero.
A nonzero source clears ZF; a zero source sets ZF and leaves the destination
unchanged. Word destinations preserve the upper register half. wasm86 clears
CF/AF/SF/OF and sets PF when the full logical source has an even number of set
bits, including a zero source. These rules follow the
[Intel instruction reference](https://cdrdv2-public.intel.com/868137/325462-089-sdm-vol-1-2abcd-3abcd-4.pdf#page=803).
Memory sources require read access to the complete
operand before any destination or flag change. Source and address registers
use their old values even when they also name the destination.

LEA (`8D`) computes the effective address encoded by ModRM/SIB and writes it to a
dword register. With `66`, it writes the low word and preserves the upper half of
the destination. Address calculation always uses full 32-bit base and index
registers and wraps at 32 bits before destination truncation. LEA preserves flags
and performs no data-memory access or page-permission checks. Its source requires
a memory addressing mode; register-mode ModRM is unsupported. LEA-only snapshots
need no guest or page-table memory imports.

CMOVcc (`0F 40`–`0F 4F`) conditionally copies a word or dword register/memory
source into a register. All sixteen conditions use the same flag queries as
SETcc and Jcc. The source is read even when the condition is false, so an untaken
move can still fault. A false condition preserves the destination; a taken word
move preserves its upper half. Both outcomes preserve flags and retire once.

Relative branches use `EB` for short JMP, `E9` for near JMP, `70`–`7F` for
short Jcc and `0F 80`–`0F 8F` for near Jcc. Short displacements are signed
bytes. Near displacements occupy a word with `66`, or a dword otherwise.
Targets are relative to the byte after the complete instruction, including
prefixes. A taken branch with `66` truncates its target to sixteen bits,
including a short branch whose displacement remains one byte. An untaken Jcc
retains the full 32-bit fallthrough address. Repeated `66` prefixes have the
same effect as one.

JECXZ (`E3`) branches when ECX is zero without changing it. LOOP (`E2`)
decrements ECX with 32-bit wrapping and branches when the result is nonzero.
LOOPE/LOOPZ (`E1`) also requires ZF set; LOOPNE/LOOPNZ (`E0`) requires ZF clear.
Both conditional forms decrement ECX even when ZF prevents the branch, and all
four instructions preserve every flag. Their signed displacement always occupies
one byte. The current 32-bit address mode selects ECX even with `66`; the prefix
only truncates a taken target to sixteen bits. An untaken branch retains the full
fallthrough EIP. These rules follow the Jcc and LOOP/LOOPcc entries in the
[Intel instruction reference](https://cdrdv2-public.intel.com/868137/325462-089-sdm-vol-1-2abcd-3abcd-4.pdf).

Near CALL uses `E8` with a relative word/dword displacement, or `FF /2` with an
absolute register/memory target. It reads the target using entry registers and
memory, then pushes the fallthrough pointer at operand width. This preserves the
original target for CALL ESP, ESP-based addresses, and source bytes overwritten
by the push. With `66`, both the saved pointer and final target use their low word.
Indirect JMP (`FF /4`) reads an absolute word/dword target without changing ESP.

Near RET (`C3`) pops a word/dword target; `C2` also discards an unsigned imm16
byte count after the pop. This immediate always occupies two bytes. ESP becomes
`entry_ESP + operand_bytes + cleanup`, wrapping at 32 bits even with `66`.
RET checks only the return-pointer cell; it does not access the discarded bytes
or validate the resulting ESP. Word indirect and return targets are zero-extended.
These rules follow the CALL, RET and JMP entries in the
[Intel instruction reference](https://cdrdv2-public.intel.com/868137/325462-089-sdm-vol-1-2abcd-3abcd-4.pdf).
Far calls, returns and jumps remain outside this flat-address subset.

JMP, Jcc and JECXZ preserve registers and flags. LOOP/LOOPcc change ECX, while
CALL and RET change ESP; all preserve the other registers and flags. Each
successful transfer retires once before dispatching the chosen EIP. All required
instruction fields are fetched before operand access or condition evaluation.
Control transfers do not fetch the
destination instruction; its fetch faults belong to the next execution entry.
Snapshot compilation stops at the transfer and never follows its destination
or decodes its fallthrough.

Group `83` sign-extends its encoded byte immediate to the operand width. SETcc
writes a byte containing 0 or 1; its ModRM.reg field is ignored.
INC, DEC, NOT and NEG have one destination and no immediate. The ModRM extension
selects both the operation and its fields: `F6`/`F7` /0 reads a TEST immediate, while
/2 through /7 finish after the register or address fields.
PUSH `68` reads an operand-sized immediate; `6A` sign-extends its encoded byte
to the operand width.

The `66` operand-size prefix selects word data; repeating it keeps that size.
Byte forms remain byte-sized with `66`. `F3` repeats MOVS/STOS; it can appear before
or after `66`, and repeated copies retain their effect. Other prefixes, including address-size
`67`, are outside the supported subset. The fifteen-byte instruction limit
includes every prefix, opcode and required operand field.

Byte register codes select AL/CL/DL/BL/AH/CH/DH/BH. Word codes select the low
sixteen bits of EAX through EDI. Byte and word writes preserve the other bits
of the parent register. Memory addresses use 32-bit ModRM/SIB base, index,
scale and displacement fields, or a 32-bit absolute offset. A0/A2 use AL;
A1/A3 use AX with `66` and EAX otherwise. Their encoded address is always four
bytes, independent of the data width.
Effective-address sums wrap at 32 bits; both frontends use flat addresses and
ignore segment bases. Blocks without explicit or implicit guest-memory access
retain just the CPU and dispatch imports.

Unprefixed MOVS, STOS, LODS, CMPS and SCAS process one byte, word or dword per instruction.
MOVS copies memory at ESI to memory at EDI; STOS stores AL/AX/EAX to memory at
EDI; LODS loads memory at ESI into AL/AX/EAX, preserving the unused upper bits.
These three transfers preserve the complete flag record. CMPS compares memory
at ESI against memory at EDI; SCAS compares AL/AX/EAX against memory at EDI.
Both set CF/PF/AF/ZF/SF/OF from the first value minus the second and preserve
the other flags.

After the element succeeds, each used index increases by the element width when
DF is clear and decreases when DF is set. MOVS and CMPS update both ESI and EDI;
LODS updates ESI; STOS and SCAS update EDI. Index arithmetic wraps at 32 bits,
including word operations. ECX is unchanged and each instruction retires once.
MOVS and CMPS check the ESI source before the EDI access. A faulting access leaves
the instruction's registers, flags and destination memory unchanged; earlier
completed instructions remain visible. Overlapping MOVS operands copy the complete
source element before storing it. These rules follow the string instruction
entries in the [Intel instruction reference](https://cdrdv2-public.intel.com/868137/325462-089-sdm-vol-1-2abcd-3abcd-4.pdf).

`F3` repeats MOVS/STOS until the full 32-bit ECX reaches zero. Zero ECX skips
data access. Each successful element advances its indices and decrements ECX;
an access fault retains the successful elements, current indices and remaining
ECX. EIP stays at the instruction's first prefix so execution can resume after
repairing the mapping. Word operand size changes the element width, while ECX,
ESI and EDI remain 32-bit. Overlap follows sequential element order, including
aliases through different virtual pages. These restart rules follow the
[Intel REP instruction entry](https://cdrdv2-public.intel.com/782151/253667-sdm-vol-2b.pdf).

A complete REP retires once, including zero-count execution; a faulting REP
does not retire. Both frontends execute all remaining elements before dispatching
the successor. REP ends a snapshot block and consumes one instruction from its
compilation limit. This scalar implementation checks each element separately.
Repeated LODS/CMPS/SCAS, `F2`, address-size overrides and segment overrides remain
unsupported. In particular, `F3` does not repeat arbitrary instructions.

PUSH and POP transfer a word or dword through a 32-bit stack pointer. PUSH reads
its source using the entry register values, then subtracts the operand size from
ESP and stores on the stack. Thus PUSH ESP stores the original ESP. POP reads the
stack, then adds the operand size to ESP; a memory destination using ESP is
addressed with that incremented value. A POP ESP register destination replaces
the incremented pointer with the popped dword. POP SP replaces its low word,
preserving the high word of the incremented ESP. These instructions preserve
the entire flag source and always require guest-memory imports.

PUSHF/PUSHFD (`9C`) and POPF/POPFD (`9D`) use the same stack accesses. `66`
selects a two-byte FLAGS image; the default is a four-byte EFLAGS image.
The flat execution subset uses a fixed user-mode flags-transfer contract:
CPL 3, IOPL 0, IF set, and VM/RF/VIF/VIP clear. PUSH sets reserved bit 1 and IF,
and copies CF/PF/AF/ZF/SF/TF/DF/OF/NT to bits 0/2/4/6/7/8/10/11/14. PUSHFD
also copies AC/ID to bits 18/21. All remaining image bits are zero.
POPF restores those nine modeled low flags and preserves AC/ID; POPFD restores
all eleven. Both ignore incoming IF, IOPL and other unmodeled image bits. These
rules follow the CPL > IOPL rows of Intel's POPF Table 4-16 and its PUSHF entry in
the [instruction reference](https://cdrdv2-public.intel.com/868137/325462-089-sdm-vol-1-2abcd-3abcd-4.pdf).
TF, NT, AC and ID can be transferred and queried; debug exceptions, task switching,
alignment checking, interrupt delivery and changing privilege modes are outside
the current execution subset. PUSHF/PUSHFD preserve the complete flag backing record.
A faulting POP preserves both ESP and the incoming flags.

Stack operations check each complete source and destination access before
changing registers or memory. Faults preserve the faulting instruction's entry
state while publishing any earlier completed instructions. When both accesses
would fault, wasm86 checks the source first; this is its deterministic access
policy. Operand size controls the two- or four-byte transfer and pointer change;
stack addresses remain 32-bit in this flat-address subset.

`compile_interpreter_step()` builds a generated `step() -> i64` entry for the
same instruction subset. Both compiler functions return a `CompiledModule`
containing WebAssembly bytes and its exported entry name.

```rust
let module = wasm86_x86::compile_interpreter_step()?;
```

The step reads EIP from CPU state and fetches the instruction from paged guest
memory. A successful five-byte contiguous-range check permits direct reads of
fields within that window; later fields use checked fetch. Otherwise the exact
path checks the opcode first and reads only required fields, checking bytes in
order when a word or dword cannot be read directly. Group instructions read ModRM
and reject unsupported extensions before fetching any SIB, displacement or immediate.
For supported forms, all instruction fields are fetched before data access is checked.
No read requests byte sixteen. A wide field crossing the limit is read byte
by byte: a missing required byte below the limit faults first. If those bytes
are available, requiring byte sixteen returns general protection with error code
zero. Snapshot decoding follows the same byte order but reports truncation
or `InstructionTooLong` as construction errors. Success uses the same instruction
semantics, state publication and dispatch as snapshot blocks.

Snapshot decoding reads supplied bytes while compiling; runtime decoding reads
guest bytes during execution. Both use the same instruction forms for opcode
patterns, physical fields and operand binding. `instruction::definitions` keeps
each instruction family's encoding table alongside its ordinary Rust semantic body:

```rust
instruction_families! {
    SHL {
        execute: shift(ShiftOp::Left);
        forms {
            0xD0 /4 => byte(rm, constant(1));
            0xD1 /4 => word_or_dword(rm, constant(1));
            0xD2 /4 => byte(rm, CL);
            0xD3 /4 => word_or_dword(rm, CL);
            0xC0 /4 => byte(rm, imm8);
            0xC1 /4 => word_or_dword(rm, imm8);
        }
    }
}
```

Operands appear in the body's argument order. `rm` selects ModRM.r/m,
`modrm_reg` selects ModRM.reg, and `/4` reserves ModRM.reg as an opcode extension.
`0x0F 0xBE` spells both bytes of an extended opcode. `+reg` covers eight opcode
register encodings and requires `opcode_reg`; `+cc` covers sixteen condition codes
and supplies the decoded condition to the body. These patterns expand into the
existing shared forms, so adding a family requires no central instruction enum or
lowering case.

`byte` fixes the data width at eight bits. `word_or_dword` selects sixteen bits with
`66` and thirty-two bits otherwise. Ordinary location tokens use that width;
`rm8`, `rm16`, and named registers such as `AH`, `CL` or `AX` give an independent width.
`accumulator` selects AL, AX or EAX at the row's width. A row can spell out both
operand-size alternatives when each argument changes differently:

```rust
0x98 => word(AX, AL) | dword(EAX, AX);
```

That is one encoding: CBW with `66`, CWDE without it. Both alternatives must decode
and bind the same physical fields. The declaration helper checks that contract,
argument arity, opcode-range boundaries and field compatibility during constant
evaluation. Catalog tests check that opcode patterns and extensions do not overlap.

`no_operands()` selects one handler regardless of the operand-size prefix:

```rust
CLC {
    execute: set(Flag::CF, false);
    forms {
        0xF8 => no_operands();
    }
}
```

Its body receives the execution builder and any fixed `execute` arguments.
The form uses the ordinary opcode-only fetch path with an empty operand binding.
Sized forms can also have no operands: `byte()` supplies `I8` to a generic body,
and `word_or_dword()` supplies `I16` or `I32`. String and stack-flag instructions
use this existing path for their implicit operands.

A family can supply a `repeat` body with the same argument and width adapters:

```rust
MOVS {
    execute: move_elements(Repetition::Once);
    effects: [memory_read, memory_write];
    repeat: move_elements(Repetition::Count);
    forms {
        0xA4 => byte();
        0xA5 => word_or_dword();
    }
}
```

`PrefixState` records the supported prefix bytes before form selection. Both decoders
use `Form::resolve(prefixes)` to accept a form and select its handler, physical fetch
widths and block boundary. For the current forms, `F3` requires the declared `repeat`
body; it is interpreted here rather than recorded as repetition by the byte cursor.
The resulting `ResolvedForm` binds the decoded fields through the existing binding API,
so lowering needs no prefix switch. Repeated forms currently require implicit operands
in the primary opcode map.

Physical immediate widths stay independent of logical data widths. `imm8` and
`imm16` consume one and two bytes respectively; `imm` follows the operand-size
attribute. `signed_imm8` consumes one byte and sign-extends it to the row's width.
`rel8` and `rel` describe branch displacement fields. `constant(1)` consumes no
bytes and gets its logical type from the body. `moffs32` always encodes a 32-bit
absolute address, regardless of data width. `address` requires memory addressing
but passes the effective address without reading data memory; LEA uses it with
the ordinary MOV body.

A family's effects appear beside its body. For example:

```rust
RET {
    execute: return_near;
    effects: [memory_read, control_transfer];
    forms {
        0xC3 => word_or_dword(constant(0));
        0xC2 => word_or_dword(imm16);
    }
}
```

The return-address cell follows the operand-size attribute while the encoded
cleanup remains unsigned 16-bit. `memory_read` and `memory_write` declare implicit
memory use for stack and string accesses; explicit memory operands already supply
it. Permissions are checked by the actual accesses. `control_transfer` ends
the block and selects the existing successor-returning handler interface. That
interface receives the bound operand, condition and fallthrough EIP, and returns
the next EIP. Fixed arguments in `execute` follow those inputs for transfer bodies,
just as they follow typed operands for ordinary bodies. For example,
`execute: loop_relative(Some(Condition::E));` adds a ZF test to the shared LOOP
body. Ordinary typed bodies return `Result<()>`; their adapters return
fallthrough after success. Execution owns retirement and state publication.

The `forms::declarations` helper derives an `Encoding` and operand bindings from
each row. `Encoding::ModRm` describes reg/rm fields and an optional trailing
immediate. Both decoders retain them in `DecodedFields::ModRm`; the immediate is
fetched after all address fields regardless of the body's argument order. Binding
assigns these fields to zero, one, two or three arguments without reading guest
state. Lowering converts snapshot literals and runtime expressions to the common
`Val<I32>` carrier and calls the bound Rust handler. The `handlers` module owns
these callable shapes and their word/dword selection.

`Input<T>` and `TypedLocation<T>` attach logical width to values and locations
while deferring access until the body requests it. A location converts to an
`Input<T>` when the body's signature requests a read-only argument. XCHG and XADD
instead take two `TypedLocation<T>` arguments; an immediate cannot satisfy that
signature. Ternary bodies likewise state the destination and source types, so
SHLD/SHRD take a word/dword destination and source with an independent byte count.

Named bindings select a parent view directly, including AH's high byte.
`NamedRegister` keeps that identity separate from register encoding, and high-byte
views require byte width. Semantic bodies use
`TypedLocation::<T>::register(Gpr32::Eax)` for the low part at width `T`.
`RegisterOperand` distinguishes named views from encoded fields, whose
width-dependent mapping includes byte codes 4–7 selecting AH/CH/DH/BH.
`TypedLocation::offset_memory` adds a wrapping byte displacement to a memory
location and leaves registers unchanged. It defers address-register reads and
access checks, so bit-string operations reuse ordinary reads and guarded updates.
The bit-test definitions own signed register offsets versus immediate offsets;
the address and memory owners retain their normal policies.

Both decoders use `DecodeState` to track the opcode map, find forms admitted by the
prefix state and choose unsupported-instruction diagnostics. A map with no admitted
forms is rejected before fetching its selector byte. With the current forms this
preserves the early rejection of `F3 0F`.

The runtime decoder owns its byte cursor, proven window and completion policy.
The cursor tracks byte position and fetch guarantees independently of prefix meaning.
Direct and checked entries start at a known position; resumed entries receive the
instruction's total consumed byte count. This choice follows the cursor's position.
Resumed entries specialize only the prefix states admitted at their decode point,
using the same form-driven opcode switch and operand handling. Only the chosen opcode
checks its ModRM extension. Prefix dispatch finishes before the execution loop starts;
element iterations never re-enter decoding.
An exact opcode case retains its register selection, so compact MOV, unary and
stack forms use fixed register views. ModRM and SIB fields select runtime views.
Memory decoding selects the SIB or ordinary layout
before reading address fields; ordinary addresses have no index term. Direct
entries retain the original fetch window across fields whose bounds fit it.
An execution builder resolves operand locations, checks memory access, and tracks
instruction progress. Typed handler operands pass their logical widths to its
`read`, `write`, `update` and `update_pair` operations. The builder's `update` checks
write permission before reading the old destination value, then runs the semantic
callback and stores its result through the same checked access. ADD, ADC, SUB,
SBB, AND, OR and XOR use this operation. CMP and TEST only read operands and set
flags, so their memory operands require no write permission.
`TypedLocation::update_pair` prepares both write targets, then passes their old
values to a callback as `PairValues { left, right }`. The callback returns the same
shape with replacements. The builder writes right before left, so the left result
wins when both locations alias. XCHG swaps the old values; XADD returns the sum and
old destination; CMPXCHG pairs its destination with `TypedLocation::register(Gpr32::Eax)`
and selects their new values after comparison. This keeps address resolution and
guest-fault checks ahead of all effects without exposing prepared targets to
instruction handlers.
A fault publishes completed definitions into its terminating branch without
consuming the parent state used by the successful path.
MOVS/STOS use `string_elements` to choose one element or a native counted loop.
The family supplies its used indices and element operation; execution carries
ECX and those indices through the compiler's typed loop. Each iteration forks
the incoming state definitions and installs its current tuple before any access.
Faults publish that state, while successful loop exits join their final register
values into the enclosing state. Forking copies construction-time definitions,
without copying CPU or guest memory. Direction-derived stride and the STOS value
are instruction invariants; element reads and stores remain inside the loop.
Address resolution accepts explicit register values for an access. POP supplies
its next ESP when preparing the destination, so the usual base, index, scale and
displacement calculation sees that value while fault publication retains entry
ESP. After both accesses pass their guards, POP defines ESP and writes the
prepared target.

The stack owner accepts typed push values, so PUSH reads its source and CALL
reads its target before using the same guarded push. POP and RET share a guarded
stack read that retains the value and next ESP without committing the pointer.
POP checks its destination before committing; RET adds its cleanup and returns
the popped value as the successor EIP.

The state value environment tracks typed byte, word and dword locations,
forwarding known definitions and caching reads. A location describes either a
fixed offset or a computed address together with its possible backing range;
reads and definitions use the same interface for both. State derives register
locations from the `CpuState` layout. Named CPU loads and stores use field paths,
with offsets and storage widths inferred from the Rust fields.
Reads through overlapping views synchronize earlier definitions to backing. A covering
write replaces superseded definitions. Computed register accesses synchronize
overlapping definitions, then invalidate potentially written locations. These
are completed effects; publication does not undo a partially executed instruction.

The `alu` module owns pure integer results and their status-flag changes.
Operations return `AluResult<T> { result, flags }`: a typed calculation result
and a `FlagChange`. Instruction handlers read operands, request an ALU
outcome, apply its flag change and write its result through checked locations.
For example, ADD uses `ArithmeticOp::Add.apply(left, right)` and AND uses
`LogicOp::And.apply(left, right)`. CMP and TEST use those same operations and
discard the destination result. ALU construction and flag queries build symbolic
expressions without a builder; they perform no CPU reads or writes.

ADD, XADD, SUB, CMP and CMPXCHG retain their arithmetic operands and result in a
typed `StatusSource` for later flag queries. Logic sources retain their result;
explicit sources retain only six symbolic flag values. `AnyStatusSource` holds a
byte, word or dword source without erasing the compiler values' logical types.
ADC and SBB use `ArithmeticOp::apply_with_carry`, with the incoming CF read after all
operand guards. They share ADD/SUB's arithmetic equations, adding CF or subtracting
it as a borrow, and return explicit flag values alongside the destination result.
The stored-record decoder reuses `ArithmeticOp::result` to reconstruct an
arithmetic source from its original operands. A same-block CMP or SUB condition
can compare those operands directly without calculating unused flags.

The `flags` modules own architectural flag identity, image encoding, write masks,
changes and condition rules. `Flag` covers the six status flags and TF/DF/NT/AC/ID;
`StatusFlag` identifies only the bits that an ALU source can supply. `FlagMask::STATUS` names that subset,
while `FlagMask::ALL` contains all eleven flags. The mask uses dense logical
indices, independent of image bit positions or backing offsets. Flag-bit extraction
uses typed truncation, so raw intermediates can remain unnormalized until their logical low bit is needed.

`ExecutionBuilder::read_flag` and `write_flag` expose the same logical interface
for every modeled flag. A write accepts a `Val<I1>` or boolean:

```rust,ignore
execution.write_flag(Flag::DF, false)?;
let carry = execution.read_flag(Flag::CF)?;
execution.write_flag(Flag::CF, carry.xor(true))?;
```

`read_flags` accepts an array of logical flags and returns their current values
in the same order, including repeated flags:

```rust,ignore
let [carry, zero] = execution.read_flags([Flag::CF, Flag::ZF])?;
```

State resolves pending changes and batches only missing stored status flags.
Local values remain expressions, and TF/DF/NT/AC/ID use their direct state. An empty request
performs no reads. The internal mask describes the requested set while generating
code; it is not passed to Wasm. Single-flag and condition reads retain their
specialized comparison paths.

`state::flags::FlagState` owns admission, queries, preservation and publication
for all eleven flags. Its status state retains a stored record or symbolic
source followed by changes in instruction order. Direct flag storage uses the
same typed environment mechanism as registers, with disjoint locations owned by
the flag state. One location mapping owns the five direct flag fields. Their reads
use current symbolic bytes without inspecting the arithmetic record. Actual writes
canonicalize each changed byte; an inactive conditional write preserves its original
raw byte. Storage offsets and widths remain inside state.

`write_flags` accepts a `FlagChange` with an explicit write mask, optional predicate
and `FlagValues`: a status source or individual architectural flag values.
Like `write_flag`, it defines symbolic state; publication writes the backing image.
Omitted flags are preserved. `change.when(predicate)` restricts when the change
applies; repeated calls combine predicates with AND. `preserving(flag)` removes
one write, while `retaining(mask)` restricts the write set. Both keep any status
recipe intact. These methods construct descriptions; state admits all supplied
values before changing either status or direct flag state. An unconditional complete
status source replaces only the earlier status history and preserves the five direct flags.

`UnaryOp::apply` defines INC, DEC, NEG and NOT. INC and DEC reuse the arithmetic
operation and `outcome.flags.preserving(StatusFlag::CF)`, which removes the CF
change without reading the old carry or expanding the remaining arithmetic recipe.
The same method can remove a flag from an already partial change. NEG uses subtraction from zero and its existing lazy
record; CF is set exactly when the original operand is nonzero. NOT returns an
inverted value and an empty flag change.

`MultiplyOp::apply` returns the full product in the same `AluResult` shape.
`DoubleWidth` maps operands to full products and dividends: `I8` to `I16`,
`I16` to `I32`, and `I32` to `I64`. The operation computes signed or unsigned
overflow and a complete flag change. Instruction handlers split that product
across the implicit result registers or truncate it for explicit IMUL forms.
The width mapping and product calculation perform no architectural accesses.

`ExecutionBuilder::divide` consumes an original double-width dividend and divisor
and returns named quotient and remainder values. It owns the sequencing of
arithmetic and guest fault exits: unsigned high-half comparison rejects zero and
overflow together; signed division checks zero and double-width MIN/-1 before
computation, then checks the quotient's destination range. Instruction handlers
own implicit register reads and writes. `fault_if` uses the existing state
publication mechanism to preserve completed instructions on the error branch.

`BitTestOp::apply` masks an offset within the logical operand, produces the
unchanged/set/reset/complemented result, and returns a partial CF change using
the old bit. BT consumes only that change after a read; the modifying forms
use the existing checked update. No bit-test operation reads the old flag source;
state composes the preserved flags when a condition or publication needs them.

`BitScanOp::apply` accepts the source and previous destination, and returns
the final destination and a complete flag change. It uses logical-width compiler
`ctz`/`clz` operations for the bit position and pure `select` expressions for
zero inputs. The instruction handler reuses source reads, checked destination
updates and flag publication; scan semantics need no new operand interface.

`ShiftOp::apply` constructs a result and six symbolic flags. Its variants name
left, logical-right and arithmetic-right shifts. `DoubleShiftOp::apply` accepts
an additional source value. Both use the shared shift result/flag construction;
double shifts keep CF defined at a count equal to the operand width.
`RotateDirection::rotate` and `rotate_through_carry` describe a partial CF/OF
change. Each semantic operation attaches its own flag condition with `when`:
RCL/RCR use the effective count after carry-ring reduction, while shifts and
ROL/ROR use the masked count. Every handler passes `outcome.flags` to `write_flags`.
Admission validates the predicate and every supplied flag value before changing
state, even for false conditions or empty changes. Constant true conditions become
unconditional changes; false conditions preserve the previous source. Runtime
conditions stay with their changes in the history. A condition query composes the
bits it needs when no one source supplies them all. A retained source can use its
comparison shortcut whenever its write mask covers every condition dependency,
including after preserving unrelated flags. Conditional values use pure selections. Stored reads stay on the owning
path so cached values remain available to later queries. Reading flags never
modifies the stored record.

At publication, state converts the current source into a `FlagRecord`: arithmetic
operands, a logical result, or six concrete status bits. Its payload variant
determines the record kind. One writer stores the payload before its kind; it does
not inspect the instruction or its incoming carry. The record exists only at this
boundary. Explicit flags are symbolic expressions; the compiler places their
evaluation where needed and leaves unused expressions unevaluated. Their array
uses logical `StatusFlag` indices; record conversion defines the CPU byte order.
Publication checks whether an active partial change survives the latest complete
replacement. If so, it composes six status bits and writes a concrete record.
Otherwise it checks complete replacements newest first and publishes the first
active source, or the base. Both paths use a flat history and bounded control
nesting. A stored base needs no writes: a zero-count shift or rotate preserves its
entire backing record. A prior local source still publishes its own record.
Publication uses a separate cache in its exit arm, leaving the live state intact.
Admission and history, queries, record conversion, and publication each have a
focused module under `state::flags`.

ADD, XADD, SUB, CMP and CMPXCHG publish zero-extended operands to CPU dwords 4 and 8,
then the kind byte at 0. SUB kinds 1/5/9 and ADD
kinds 2/6/10 denote byte/word/dword operands. ADC/SBB sources instead publish all
six concrete flag bytes, then kind 0: the stored two-operand format cannot retain
an incoming carry. Their unused payload dwords remain untouched.
AND, OR, XOR and TEST retain only the logical result. Their records use kinds 3/7/11
with the zero-extended result at offset 4; offset 8 is unused and remains untouched.
The state owner retains the current source choices and writes one record at publication;
replacing a source does not schedule or repair individual field writes. Logic clears
CF/OF as required by x86. Its AF value is architecturally undefined; wasm86 chooses
zero. This deterministic choice avoids retaining or evaluating the old flag source.

An undefined flag still produces a normal bit when read; it does not authorize a
trap or make the guest instruction undefined behavior. An unaffected flag must
retain its previous logical value. Implementation choices for undefined flags are
made per instruction family, shared by interpreter and blocks, and documented
separately from architectural guarantees. They do not promise to reproduce a
particular physical CPU's undocumented behavior. Architecture comparisons exclude
undefined flag values; separate policy tests may assert wasm86's chosen value.
A nonzero kind owns all six status flags, so concrete flag bytes may be stale.
Kind 0 reads the low bit of CF/PF/AF/ZF/SF/OF bytes 12–17. Publication writes
canonical bytes containing 0 or 1; reads also accept noncanonical input bytes.
Valid record kinds are an internal invariant. A stored direct query selects its
exact record kind before reading its typed inputs. Subtraction relations compare
the original operands; logical zero/nonzero queries compare only the result.
Other queries use shared readonly readers. Inverse conditions share a reader and
cached result. Partial changes request missing inherited flags together as separate
I1 results, sharing one record decoder and invocation. Readers are created only when needed;
queries preserve the stored representation.
CLC, STC and CMC use the common flag interface to replace only CF. CMC reads the
current carry with `read_flag(Flag::CF)`; constants and computed bits use the same
change representation as CLD/STD's DF writes. Publication resolves the other five flags
and writes the concrete kind-0 format, preserving unused payload bytes.
`flags::image::FlagImage<N>` describes an architectural image with an ordered flag
roster, bit positions and fixed bits. AH, FLAGS and EFLAGS images share pure packing
and extraction. Packing accepts the results of `read_flags`; extraction produces
one `FlagChange` for `write_flags`. The word roster omits AC/ID entirely, so those
flags require no read or write for a word transfer.
LAHF and SAHF retain ordinary unary `byte(AH)` declarations. PUSHF/POPF use
`word_or_dword()` declarations with no operands; the declaration adapter supplies
the semantic body's type argument from the selected width. The existing nullary
handlers and both decoders already carry that width selection. Their bodies reuse
the guarded stack `push` and `pop_value` operations.
The instruction bodies do not depend on the backing flag record's representation.
MOV, MOVZX, MOVSX, LEA, XCHG, CMOVcc and SETcc preserve flags, and these instructions
leave control and system flag bytes untouched. A faulting operand access preserves the
previous instruction's flags.

The step and snapshot blocks with memory operands also import `wasm86.guest`
(minimum one Wasm page) and `wasm86.machine` (minimum 64 pages, or 4 MiB).
All three memory imports require distinct backing objects. Machine memory starts
with 2^20 little-endian 32-bit page-table entries: bit 0 marks presence, bit 1
permits writes, and bits 12–31 identify a 4-KiB frame in guest memory. Data reads
require presence. Present frames must fit the backing RAM; this is an internal
invariant. Unexpected host or Wasm traps from inconsistent internal state are not
part of the guest execution contract.

DIV/IDIV divide error returns `1 << 48`, with no error code or address payload.
Earlier instructions remain published; the division preserves its entry state
and EIP, does not retire, and does not dispatch.

A missing instruction page returns the 64-bit word
`(4 << 48) | (0x10 << 32) | first_unavailable_address`. A data fault returns
`(4 << 48) | (error << 32) | first_denied_address`, where error bit 1 identifies a
write and bit 0 identifies a present but denied page. A word or dword data range
that crosses `0xffffffff` is rejected at its start with read error 0 or write error 2;
this is the current address-space policy. A one-byte access at that address fits
without consulting another page. Instruction fetch instead wraps.
All pages are checked before a data store writes any byte, including scattered
physical backing. A fault publishes earlier completed instructions and leaves EIP
at the faulting instruction; the failed instruction does not retire or dispatch.
Contiguous accesses use native loads and stores. Scattered accesses call a shared
helper for their logical width and direction, resolving each byte's physical
address after the full span passes permission checks. Memory creates each helper
when first needed and shares it across the module's functions; byte accesses need
no transfer helper.

Memory checks natural alignment first for multi-byte data accesses. Aligned
accesses and unaligned spans within one page share the ordinary permission check;
only a crossing span calls the range resolver. Successful resolution returns a
physical address and a separate logical bit for scattered backing. Denials join
one fault callback, which must terminate before a checked access can be returned.
The execution builder uses that callback to publish state once per access fault.
Page-table translation owns page facts and range-wrap priority; memory access
control owns the successful and faulting paths.

An unsupported instruction form returns `(8 << 48) | (opcode << 32) | instruction_eip`.
The opcode field contains the first byte after any `66` prefixes, including `0F`
for an extended opcode. An unsupported form following `F3` reports `F3`. This reports
the implementation's unsupported subset, not an architectural invalid-opcode
fault. An instruction requiring more than fifteen bytes returns `2 << 48`, the
zero-error general-protection word, without retiring or dispatching. The step
executes one instruction and has no instruction-budget, segment or run-loop
behavior.

`wasm86-compiler` builds WebAssembly functions from integer constants,
parameters and typed integer expressions. `Val<T>` represents either a standalone
literal or an expression belonging to a function body. Values such as `Val<I1>`
and `Val<I32>` carry logical integer types; function signatures use the
corresponding `Type` variants.
`Signature.results` lists the logical return types in order: `vec![Type::I32]`
returns one integer, `vec![Type::I1, Type::I1]` returns two flags, and `vec![]`
returns no values. Parameters also use logical integer types.
Supported integer sizes are 1, 8, 16, 32 and 64 bits.

Create standalone values with standard Rust conversions, such as
`Val::<I32>::from(7)` or `let cleared: Val<I1> = false.into();`. Fluent operations
and typed builder operands accept `Into<Val<T>>`, so `value.add(1)` and
`body.store::<I8>(memory, 12, 9)` accept native literals directly. Signed `i32` and
unsigned `u32` literals convert to any logical integer type; `bool` converts only
to `Val<I1>`, and `u64` only to `Val<I64>`. Negative `i32` literals sign-extend to
I64, `u32` literals zero-extend, and narrower targets retain the low bits.

Standalone literals can be used in different function bodies. Once an expression
uses a body-owned value, it remains bound to that body even if it folds to a
constant. Folding also preserves the branch visibility required by every original
operand. These admission checks are separate from runtime data dependencies, so
a legal constant fold can omit reads it no longer needs.
`body.value::<I32>(operand)?` admits a value into a body
and checks ownership and scope immediately. Other expression construction errors
are reported when the value is consumed. Calling `body.return_(&value)` completes
the function body; shared expressions use reusable WebAssembly locals. A raw
literal can take its logical type from the return signature, so `body.return_(7)`
is valid too.

Use `program.function(signature, |body| { ... })?` to declare and complete a
function together. The handle is returned only after the callback completes its
body. An error or missing completion discards the function and resources added
by its callback, and restores earlier forward declarations it completed. Discard
handles created by a failed callback. Separate `declare` and `define` remain
available for forward references and recursion. An open body can create a needed
helper through `body.program()`; the active function cannot be reopened.

Expressions support wrapping addition, subtraction and multiplication (`value.mul(3)`),
bitwise `and`/`or`/`xor`,
logical-width `popcnt`/`clz`/`ctz`, `shl`, and `eq`/`ne` predicates that return
`Val<I1>`. Bit counts retain the input's logical type; `clz` and `ctz` count
leading and trailing zero bits and return the logical width for zero input.
For example, `Val::<I16>::from(0).ctz()` is sixteen. The borrowed `unsigned()` view
provides `shr`, `lt`, `ge` and zero extension, for example
`byte.unsigned().extend::<I32>().shl(8)`. `truncate::<I8>()` retains the low eight
bits. The borrowed `signed()` view provides arithmetic `shr`, `lt`/`ge` comparisons and sign extension, such as
`displacement.signed().extend::<I32>()`. Rust checks conversion direction;
conversions to the same type are allowed. All shifts accept an I32 value or a
literal count, for example `byte.signed().shr(count)`.

Shift counts are modulo 32 for I1/I8/I16/I32 and modulo 64 for I64. Left and unsigned
right shifts of an I8 by 8 produce zero; shifting it by 32 preserves its value. Comparisons,
right shifts and widening read the logical low bits, including after arithmetic
that overflows a narrow type. Arithmetic right shift repeats the logical sign bit;
it does not interpret unused carrier bits as part of the operand.
`value.rotl(count)` and `value.rotr(count)` accept the same I32-or-literal count
interface, but rotate modulo the logical width: 1, 8, 16, 32 or 64 bits. For example,
`byte.rotl(8)` preserves the byte. Wide rotations use native Wasm operations;
narrow rotations combine shifts within the logical width.

The signed and unsigned views also provide `div` and `rem`, for example
`dividend.signed().div(divisor)`. They use the same value expressions, literal
operands, folding and placement as other integer operations. Signed division
truncates toward zero; signed remainder follows the dividend's sign. Native
arithmetic reads the logical operands, and narrow results retain their low bits.
The compiler does not add guest exception checks. The x86 execution layer checks
zero divisors and quotient overflow, returning guest divide errors through the
CPU ABI. An unexpected native arithmetic trap is an implementation bug.

Build conditional code with the same builder methods:

```rust
body.if_(value.eq(0), |mut branch| {
    branch.store::<I32>(memory, 12, 9)?;
    branch.return_(7)
})?;
body.return_(value.add(1))?;
```

A branch can load, store, contain nested `if_` calls, return, tail-call or trap. Ending
its closure with `Ok(())` without a terminal lets it fall through. A false
condition skips the branch. `body.if_else(condition, then_arm, else_arm)` builds
two ordinary branches; each can fall through or terminate the function.
Child reads, call results, joined values and expressions depending on them cannot
be consumed outside that child; pure
expressions from parent values remain usable. Reads preserve snapshots across
conditional stores, which can require capturing a read before the condition.

Use `condition.select(when_true, when_false)` for a pure value choice. Both
alternatives are eager inputs, so select does not guard a load or call. Shared
inputs and the selection itself follow normal value placement. Both alternatives
must satisfy ownership and scope rules, including an unused alternative. Two literals can
specify their type with `condition.select::<I32>(7, 9)`.

Use `switch` to execute one case selected by an integer value. The callback receives
`Some(key)` for a listed case and `None` for the default. Case keys must be unique
and fit the selector's logical type. Each case can fall through, return, tail-call
or trap; falling through continues after the switch.

```rust
body.switch(&opcode, &[0x28, 0x85], |arm, key| match key {
    Some(0x28) => arm.return_(1),
    Some(0x85) => arm.return_(2),
    _ => arm.return_(0),
})?;
```

`switch_value::<I32, _>` instead returns a value. Every case, including the default,
must yield, branch to an enclosing block, return, tail-call or trap, and at least
one must yield. Child values obey
the same scope rules as conditional arms. Dense case ranges use a Wasm branch
table; sparse ranges use balanced comparisons without allocating a large table.
Nontrapping calculations can be placed separately in mutually exclusive arms.
Their memory reads, call results and joined inputs retain their snapshots and
sharing rules. Nested branches within one arm share its capture, and a use after
the join retains a common capture.

A comparison used only as an `if_` condition inside two separately guarded regions
can also be calculated within each region. This permits at most one additional
primitive test; its operands keep their ordinary sharing. Transparent blocks do
not count as guards. This keeps a conditional check behind its guards without
duplicating an entire shared calculation or reloading a saved memory value.

Use `if_value` to execute only the selected branch and obtain its value:

```rust
let branch_value = body.if_value::<I32>(value.eq(0),
    |arm| arm.yield_(7),
    |arm| arm.yield_(value.add(1)),
)?;
body.return_(branch_value.add(2))?;
```

`yield_` consumes the direct value-arm builder and supplies the conditional's
result; `return_` still returns from the whole function. For a nonempty result,
every arm must yield, branch to an enclosing block, return, tail-call or trap,
and at least one arm must yield. Arm effects remain ordered,
while an unused yielded expression and its possible traps can disappear. A join
preserves raw intermediate bits; narrow values are normalized when an operation
or function boundary needs their logical low bits. Construction errors discard
both arms and leave the parent usable.

Calls, conditionals, switches and blocks use the same logical `Results` shape. A scalar
marker such as `I32` returns `Val<I32>`; a tuple such as `(I32, I1)` returns
`(Val<I32>, Val<I1>)`; `[I1; 4]` returns four typed flags. Shapes can be nested.
`()` has no results and allows control blocks to fall through without a yield.
Control-block components are checked and placed independently, so an unused component
does not force its load or pure call to execute. Native literals passed to
`return_`, `yield_` or `branch` take their types from the result shape, like call
arguments. These methods also accept tuples, arrays and vectors of scalar arguments.

Use `block` when nested paths need to leave one enclosing region with results.
This example assumes an I64-returning function, I32 addresses and I1 conditions:

```rust
let (address, present) = body.block::<(I32, I1)>(|mut checks, denied| {
    checks.branch_if(&first_missing, &denied, (&start, false))?;
    checks.branch_if(&second_read_only, &denied, (&next_page, true))?;
    checks.return_(0)
})?;
body.return_(address.unsigned().extend::<I64>()
    .or(present.unsigned().extend::<I64>().shl(32)))?;
```

`Label<R>` names the block's exit. `branch` consumes its active builder,
passes the declared values and skips the rest of that block. The direct block
body can also use `yield_`. The conditional forms `branch_if` and `yield_if`
transfer their values only when the condition is true and leave the builder open
for the false path. They use the same destinations and argument checks as their
unconditional forms. `yield_if` uses the direct body's result destination;
nested ordinary `if_` branches use an explicit label. Labels belong to one function
body and remain usable
only in their block and its descendants; keeping a clone cannot extend that
scope or revive a discarded block. Nonempty blocks require complete paths and
at least one incoming result. Wasm multi-value signatures and branch depths stay
inside the compiler.

`loop_::<P, R>` gives a loop separate input and result shapes. Initial values
supply its first iteration; branching to `labels.again` supplies the next one.
`labels.exit` supplies the result, including from nested branches:

```rust
let sum = body.loop_::<(I32, I32), I32>(
    (5, 0),
    |mut iteration, labels, (remaining, sum)| {
        iteration.yield_if(remaining.eq(0), &sum)?;
        iteration.branch(&labels.again, (remaining.sub(1), sum.add(remaining)))
    },
)?;
```

Direct `yield_` also completes the loop, and a unit result may fall through.
Each backedge evaluates the complete replacement tuple before binding the next
iteration's inputs, so swaps preserve both old values. Inputs and labels stay
within the loop and its descendants; outputs follow the ordinary result-join
rules. Initial and backedge arguments retain logical types without implicitly
clearing upper carrier bits. Loop inputs use conservative bounds, so later logical
observers still normalize narrow values correctly. All carried components are
retained initially. Pre-loop load and read-only call snapshots survive later
iteration writes; reads authored inside the loop observe each iteration's state.
Native Wasm parameters and results carry these edges, and local allocation keeps
outer values alive across backedges.

The compiler records conditional transfers directly. Their taken edge has its own
region, so result values keep their conditional demand and memory snapshot rules.
The condition runs before captures shared with the continuation. A conditional
transfer followed immediately by an unconditional branch can share one tuple when
both destinations need the same live values. Lowering chooses branch polarity
around the actual fallthrough destination and emits `br_if`; the untaken path
retains the tuple for its result. Other conditional transfers keep their argument
evaluation guarded. Direct `yield_` and explicit `branch` use the same internal
branch representation, including inside conditional transfers.

`body.trap()` consumes its builder and ends that execution path with a WebAssembly
trap, regardless of the function result type.

Functions can also finish with `body.tail_call(target, &[value.argument()])`.
The target may be imported or defined. Use `Val::argument()` or `.into()` to
combine values and literals in one `Argument` list; the call checks them against
its runtime signature. A `Val<T>` retains its logical type in this list, including
a standalone literal: `Val::<I8>::from(7)` still requires an I8 parameter. A raw
primitive such as `7.into()` instead takes its logical type from the corresponding
parameter. Returns follow the same distinction.
`Program::import_function` takes a `FunctionImport` containing the module name,
field name and logical `Signature`. Unused function imports are omitted; a direct
export also retains an imported function.

For a call that returns to the current function, use
`body.call::<I32>(helper, &[value.argument(), 7.into()])?`. Its typed result can
be shared by later expressions. A result created inside a branch stays within
that branch and its descendants. Multiple results use the same call API:

```rust
let (carry, overflow) = body.call::<(I1, I1)>(flag_reader, &[])?;
body.return_((carry, overflow))?;
```

All components belong to one invocation, which runs once wherever its results
are needed. When a call runs, its callee evaluates every declared result even
if the caller discards some components. The compiler conservatively infers which
memory bytes defined helpers may read or write. Helpers without inferred writes
or unknown effects may be deferred or omitted when unused, including their
arguments and possible traps. Calls that may write, call imports or reach
unresolved recursion execute in authored order even when unused.

For a function with no result, use `body.call::<()>(target, arguments)?` and finish
its definition with `body.return_(())`. The same inferred effects determine
whether the invocation must execute; calls without writes or unknown effects are
omitted, including their arguments and possible traps. They have no value to discard:

```rust
let writer = program.function(Signature {
    parameters: vec![Type::I32],
    results: vec![],
}, |mut body| {
    let value = body.parameter::<I32>(0)?;
    body.store(memory, 12, value)?;
    body.return_(())
})?;
// In another function:
body.call::<()>(writer, &[7.into()])?;
```

Calls require the complete requested result shape to match the callee's logical
signature. Returns obey the containing function's signature, and a tail call
requires the same ordered result types.

WebAssembly carries 1-, 8- and 16-bit integers in `i32`. Narrow arguments must have
their unused upper bits clear, and returned narrow values satisfy the same rule.
A one-bit argument or result is therefore `0` or `1`. The compiler normalizes
narrow call arguments once per shared value without masking intermediate
arithmetic. Imported functions must return narrow results with their unused
upper bits clear too, including when the import is exported directly.
When bit bounds prove that a value is already zero or one, testing it for nonzero
reuses that value as a logical bit without another Boolean calculation.
An unshared nonzero test used only by `if_`, `if_else`, `if_value` or `select`
can pass its normalized `i32` input directly to Wasm's truth test. Numeric Boolean
uses and switch keys retain their exact zero-or-one value. Narrow inputs still
observe their logical width, and `i64` tests still inspect all 64 bits.

Fixed-offset memory access supports `I8`, `I16`, `I32` and `I64`, using 1, 2, 4
and 8 bytes respectively. `body.load::<I32>(memory, offset)` reads a snapshot;
`body.store(memory, offset, &value)` writes the value at that point in the body.
Literal operands work directly: `body.store::<I32>(memory, 12, 9)`.
Stores keep their authored order, used loads preserve their value across writes,
and unused loads are omitted. Distinct memory declarations require distinct
backing memories.

For computed addresses, use `body.load_at::<I8>(memory, &address, offset)` and
`body.store_at(memory, &address, offset, &value)`, where `address` is a `Val<I32>`
or an integer literal.
The unsigned address plus the constant displacement does not wrap. To wrap an
address calculation at 32 bits, build it explicitly with `address.add(amount)`.
Reads preserve their snapshots even when their addresses depend on other reads;
unused reads and their unused address computations are omitted.

The ordinary test suite executes generated Wasm in Wasmtime directly from Rust.
Cases supply typed inputs and independently specified expectations. The compiler
test host observes returned values, traps, host calls and imported memories. The
x86 test host observes CPU state and guest memory at dispatch and return, including
fault exits and repeated calls on the same instance. Each independent case starts
with fresh memories and runtime state; suites reuse compiled modules where possible.
Node.js is not required for these checks:

```sh
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo fmt --all -- --check
```

The explicit V8/TurboFan lane runs every shared instruction case and sequence,
plus focused compiler and publication tests. These ignored tests require Node.js
24 on `PATH`:

```sh
cargo test --workspace --locked -- --ignored
```

The compiler groups its integration suites into one test executable. The x86
instruction suites run in the library test target so they can observe logical
flags through the private state reader without adding a public testing API. The
internal `wasm86-test-support` crate supplies shared engine configuration, integer
carriers, outcomes and V8 process transport; it is a development dependency of
the product crates. The compiler and x86 hosts own their distinct imports and
observations. Tests are ordinary named `#[test]` functions: build a module or
machine image, execute it, and assert returned values or state. There is no engine
selection to configure when adding a case. Only the V8 tests use JSON
transport, with decimal strings for 64-bit integers to preserve their bits.

Compiler behavior tests use `Fixture` to keep imported memories and callbacks
together with their host configuration. Its single-function path uses the real
compiler builder, then handles exporting and compiling. An instance accepts Rust
arguments and returns Rust values; memories and recorded callbacks are available
for direct assertions. For example:

```rust
#[test]
fn byte_addition_wraps() {
    let module = Fixture::new().function(&[Type::I8], &[Type::I8], |body| {
        let value = body.parameter::<I8>(0)?;
        body.return_(value.add(1))
    });
    let mut instance = module.instantiate();
    assert_eq!(instance.call::<i32>((255,)).unwrap(), 0);
}
```

Use the fixture's `program` for multiple functions or explicit declarations, and
inspect `module.bytes()` when generated Wasm is the subject of the assertion.
Instantiation starts fresh state; further calls on that instance retain its state.

For one instruction, add named `InstructionCase` values to its suite. Each case
supplies encoding bytes, its flag contract, and the registers or memory it uses.
`new` states all six initial flags and all six expected flag rules. Inputs and
expected results stay together:

```rust
use wasm86_x86::Gpr32::{Eax, Ebx};
use crate::support::cases::{
    test_cases, FlagExpectation::{Clear, Set}, Flags, InstructionCase as Case,
};

fn cases() -> Vec<Case> {
    vec![
        Case::new(
            "ADD AL,BL: signed overflow, upper EAX preserved",
            &[0x00, 0xd8],
            Flags { cf: true, pf: true, af: false, zf: true, sf: false, of: false },
            Flags { cf: Clear, pf: Clear, af: Set, zf: Clear, sf: Set, of: Set },
        )
        .register(Eax, 0x4433_227f, 0x4433_2280)
        .initial_register(Ebx, 1),
    ]
}

test_cases!(add_flags, cases());
```

`register` gives the full parent register's input and output, so narrow writes
also check untouched upper bits. Unlisted registers and memory must stay unchanged;
`initial_register` supplies a preserved input. Initialize memory with
`memory(address, bytes, permissions)` and give changed bytes with `expect_memory`.
Flags use `Set`, `Clear`, `Preserved` or `Undefined`. Use `DefinedBits` for a
partially undefined register result or `undefined_memory` for an undefined byte
span; the surrounding state is still checked. Share named flag values within a
case group when several cases have the same rules. Expected instruction results
are literals or independently derived data, never calculated by the runner.

The registration creates ordinary Cargo tests for Wasmtime and the explicit V8
lane. Both run every case through the interpreter and a snapshot block, starting
from fresh state, and check registers, memory, logical flags, retirement and exit.
Failure messages identify the case, engine, frontend and mismatched field.
Logical flags are read from a copy through the existing state reader; observing
them must leave every CPU byte unchanged. Raw flag-record layout and undefined
flag policy belong in their separate tests.

For instructions that do not inspect flags, `preserving_flags(name, code)`
requires the entire incoming record to stay unchanged. `replacing_flags` accepts
an opaque incoming record and explicit logical outputs; those outputs cannot
claim `Preserved`. Use `stored_flags(record)` to choose an incoming representation.
When logical initial flags are supplied, the runner verifies that the record
represents them. `preserve_flag_record()` additionally requires unchanged record
bytes for a case that reads logical flags, such as CMOV.
`expect_direct_flag(flag, bool)` changes the expected TF, DF, NT, AC or ID byte
on an instruction case or sequence checkpoint. Omitted direct flags and reserved
bytes stay unchanged; record-preserving checks allow only the explicitly stated
direct flag changes. Status flags use the case's logical flag expectations.

Cases start at `0x1000` and expect one retired instruction with fallthrough
dispatch. Use `at(origin)`, `instruction_count(count)`, `dispatch(target)`,
`fault(address, error)` or `divide_error()` to state different boundaries.
Code can cross pages or wrap EIP. A fault expects the entry EIP, no retirement and
no dispatch.
For scattered pages and physical canaries, use `map_page(page, frame, permissions)`
and `backing(offset, bytes)`. Every physical byte outside an expected write must
remain unchanged, and setup cannot silently replace the instruction encoding.

Use `SequenceCase` with one `Checkpoint` per instruction. Each checkpoint states
only its changed outputs; the runner checks every interpreter boundary and the
compiled block's final state. For example:

```rust
use crate::support::sequences::{test_sequences, Checkpoint, SequenceCase};

fn sequences() -> Vec<SequenceCase> {
    vec![SequenceCase::preserving_flags("partial writes survive a later store fault")
        .initial_register(Eax, 0x4433_2211)
        .initial_register(Ebx, 0x4000)
        .step(Checkpoint::preserving_flags(&[0x66, 0xb8, 0x34, 0x12])
            .register(Eax, 0x4433_1234))
        .step(Checkpoint::preserving_flags(&[0xb4, 0x7f])
            .register(Eax, 0x4433_7f34))
        .step(Checkpoint::preserving_flags(&[0x89, 0x03]).fault(0x4000, 2))]
}

test_sequences!(partial_writes_before_fault, sequences());
```

`SequenceCase::new` states logical initial flags; `from_opaque_flags` leaves them
unspecified until a checkpoint replaces them. Use `Checkpoint::new` for explicit
flag rules. A fault or branch ends the sequence. `trailing_code(bytes, count)`
can retain a compiled suffix after that boundary to check that it does not run.
Overlapping writes and undefined spans compose in instruction order.

Keep independent mathematical models, malformed encoding checks, external ABI,
and exact publication or generated-code assertions in focused tests using the
lower-level `Image` and observation APIs. Their names should identify that contract.

Add cases and a `test_cases!` or `test_sequences!` registration in the relevant
`tests/suites` module.
Use a named test for other scenarios, or place it beside the component
when it needs private APIs. Cargo filters select that test directly, for example
`cargo test -p wasm86-compiler test_name`. Keep related boundary values together
when they check one behavior; give distinct scenarios their own test functions.

Execution cases check behavior against literal or independently derived results;
agreement between the two x86 frontends alone is insufficient because they share
lowering. Separate ABI and code-generation tests protect external layouts,
signatures and deliberate optimization properties. Keep guest performance
measurements in V8, separate from correctness tests. Compare generated bytes
before benchmarking a change; Wasmtime correctness results do not measure the
target engine's optimization decisions or execution speed.
