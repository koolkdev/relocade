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

This crate currently supplies guest register programming. Memory transfers and
PC initialization are separate changes. The secondary controller, cascading,
memory-to-memory transfers and electrical bus timing are outside this scope.

Register behavior follows the [Intel 8237A datasheet](https://www.pcjs.org/documents/datasheets/intel/INTEL_8237A_DMA.pdf),
sections Register Description and Software Commands. Page wiring follows the
[IBM PC/AT Technical Reference](https://www.minuszerodegrees.net/manuals/IBM_5170_Technical_Reference_6280070_SEP85.pdf),
System Board pages 1-9 through 1-11.
