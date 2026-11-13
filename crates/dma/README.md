# Primary DMA controller

`relocade-dma` implements the primary 8237A register interface and PC/AT page
registers for channels 0–3. `Dma::initialize(&runner).await` binds the device on
the existing execution owner. All channels start masked.

Address and count registers use one shared low/high byte flip-flop. Each write
updates the selected byte in both the base and current register; reads return
the current register. Page registers supply address bits 16–23 independently of
the 16-bit address. Port constants name every implemented register.

Master clear resets command, requests, terminal-count status and the byte
flip-flop, and masks every channel. It preserves address/count, mode and external
page registers. Unsupported reads return open bus. Ports accept bytes only.

An emulated peripheral calls `read(channel, &memory_view, &mut bytes)` to consume
a block from the guest-programmed buffer. The call requests service directly;
the device does not need to drive a request line for each sample. It returns the
number of bytes transferred and leaves any remaining output untouched. The
operation honors masks, controller disable and single-transfer memory-read mode,
reads live mapped memory in contiguous chunks, and updates address/count at
wrap and completion boundaries. The peripheral determines how many bytes are due.

Timed peripherals register a service with `attach_channel(channel, callback)`.
DMA calls these services before register reads and writes, so elapsed transfers
use the old settings and guest reads observe current progress. Callbacks receive
the active I/O context and run without a controller borrow; they can use the
same block-read API. Channel wiring survives guest controller reset.

For low-level request-line use, `set_request(channel, true)` and
`read_memory(channel, &memory_view)` transfer one byte through the same mechanism.
The peripheral lowers that line when it no longer needs service. Block reads
preserve its state. Both APIs use the registers exposed to guest ports; there is
no separate buffer position, timer or worker.

The count encodes transfers minus one: zero transfers one byte, and `0xffff`
transfers 65,536. The 16-bit address wraps without changing the page register.
Terminal count sets the channel's status bit. In auto-initialize mode it reloads
the base address and count without masking the channel; otherwise it masks the
channel. It clears software requests but leaves the peripheral's request line
alone. Status reads clear terminal-count bits while still reporting active
requests, including masked ones. Supply a view with I/O routing for buffers that
can reach device memory.

PC initializes the controller regardless of sound-card selection and exposes
it through `pc.dma()`. A peripheral owns its request timing; DMA does not infer
completion interrupts or an audio sampling rate from its transfer count.

The secondary controller, cascading, demand/block transfers, device-to-memory
and verify transfers, servicing software requests, memory-to-memory transfers
and electrical bus timing are outside this scope. Unsupported transfer modes
consume no bytes and leave the channel's position unchanged.

Register behavior follows the [Intel 8237A datasheet](https://www.pcjs.org/documents/datasheets/intel/INTEL_8237A_DMA.pdf),
sections Register Description and Software Commands. Page wiring follows the
[IBM PC/AT Technical Reference](https://www.minuszerodegrees.net/manuals/IBM_5170_Technical_Reference_6280070_SEP85.pdf),
System Board pages 1-9 through 1-11.
