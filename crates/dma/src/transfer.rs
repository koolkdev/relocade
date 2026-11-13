use crate::{set_bit, Dma};
use relocade_machine::MemoryView;

const MEMORY_TO_MEMORY: u8 = 0x01;
const CONTROLLER_DISABLED: u8 = 0x04;
const TRANSFER_TYPE_MASK: u8 = 0x0c;
const MEMORY_READ: u8 = 0x08;
const AUTO_INITIALIZE: u8 = 0x10;
const DECREMENT_ADDRESS: u8 = 0x20;
const TRANSFER_MODE_MASK: u8 = 0xc0;
const SINGLE_TRANSFER: u8 = 0x40;

impl Dma {
    /// Sets the peripheral's logical DMA request, independently of its mask.
    /// Keep the request active until the peripheral no longer needs service.
    /// Channel numbers are 0–3.
    pub fn set_request(&mut self, channel: u8, active: bool) {
        assert!(channel < 4, "primary DMA channel");
        set_bit(&mut self.hardware_requests, channel, active);
    }

    /// Transfers one byte from guest memory to a requesting peripheral.
    /// Returns `None` when masked, disabled, not requested or in an unsupported
    /// mode. The peripheral decides when it is ready to accept another byte.
    /// Supply a memory view with device routing if the address can reach MMIO.
    pub fn read_memory(&mut self, number: u8, memory: &MemoryView<'_>) -> Option<u8> {
        assert!(number < 4, "primary DMA channel");
        if self.hardware_requests & (1 << number) == 0 {
            return None;
        }
        let mut byte = [0];
        (self.read(number, memory, &mut byte) == 1).then_some(byte[0])
    }

    /// Reads a block from the guest-programmed channel into a peripheral buffer.
    /// The call itself requests service; it does not require or change the
    /// persistent request line controlled by [`Self::set_request`].
    /// Returns the bytes transferred, leaving the rest of `bytes` untouched.
    /// Masked, disabled and unsupported channels return zero. The peripheral
    /// chooses how many bytes are due; DMA owns address/count and completion.
    /// Auto-initialize reloads and continues filling the buffer.
    /// Supply a memory view with device routing if the address can reach MMIO.
    pub fn read(&mut self, number: u8, memory: &MemoryView<'_>, bytes: &mut [u8]) -> usize {
        let channel = &mut self.channels[usize::from(number)];
        let bit = 1 << number;
        if self.command & (MEMORY_TO_MEMORY | CONTROLLER_DISABLED) != 0
            || self.mask & bit != 0
            || channel.mode & TRANSFER_TYPE_MASK != MEMORY_READ
            || channel.mode & TRANSFER_MODE_MASK != SINGLE_TRANSFER
        {
            return 0;
        }
        let decrement = channel.mode & DECREMENT_ADDRESS != 0;
        let mut transferred = 0;
        while transferred < bytes.len() {
            let remaining = usize::from(channel.count.current) + 1;
            let before_wrap = if decrement {
                usize::from(channel.address.current) + 1
            } else {
                0x10000 - usize::from(channel.address.current)
            };
            let count = (bytes.len() - transferred).min(remaining).min(before_wrap);
            let part = &mut bytes[transferred..transferred + count];
            let address = (u32::from(channel.page) << 16) | u32::from(channel.address.current);
            if decrement {
                // Preserve descending access order for mapped devices.
                for (index, byte) in part.iter_mut().enumerate() {
                    *byte = memory
                        .read_u8(address - index as u32)
                        .expect("24-bit DMA address fits the guest address space");
                }
                channel.address.current = channel.address.current.wrapping_sub(count as u16);
            } else {
                memory
                    .read(address, part)
                    .expect("24-bit DMA address fits the guest address space");
                channel.address.current = channel.address.current.wrapping_add(count as u16);
            }
            channel.count.current = channel.count.current.wrapping_sub(count as u16);
            transferred += count;
            if count == remaining {
                self.terminal_count |= bit;
                self.software_requests &= !bit;
                if channel.mode & AUTO_INITIALIZE != 0 {
                    channel.address.current = channel.address.base;
                    channel.count.current = channel.count.base;
                } else {
                    self.mask |= bit;
                    break;
                }
            }
        }
        transferred
    }
}
