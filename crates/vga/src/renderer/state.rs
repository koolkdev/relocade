//! Captured register bytes, palette and geometry shared with the shader.
use crate::{DisplayGeometry, DisplaySnapshot};

// The shader's State layout: padded raw banks, controls, geometry, then DAC RGB.
const HEADER_BYTES: usize = 128;
const PALETTE_BYTES: usize = 256 * 3;

pub(super) struct GpuState {
    buffer: wgpu::Buffer,
    pub binding: wgpu::BindGroup,
}

impl GpuState {
    pub fn new(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        memory: &wgpu::Buffer,
    ) -> Self {
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("captured VGA registers and palette"),
            size: (HEADER_BYTES + PALETTE_BYTES) as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let binding = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("captured VGA resources"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: memory.as_entire_binding(),
                },
            ],
        });
        Self { buffer, binding }
    }

    pub fn upload(
        &mut self,
        queue: &wgpu::Queue,
        snapshot: &DisplaySnapshot,
        geometry: DisplayGeometry,
    ) {
        queue.write_buffer(&self.buffer, 0, &header(snapshot, geometry));
        queue.write_buffer(
            &self.buffer,
            HEADER_BYTES as u64,
            snapshot.registers.dac.colors.as_flattened(),
        );
    }
}

fn header(snapshot: &DisplaySnapshot, geometry: DisplayGeometry) -> [u8; HEADER_BYTES] {
    let r = &snapshot.registers;
    let mut bytes = [0; HEADER_BYTES];
    for (offset, bank) in [
        (0, r.sequencer.values.as_slice()),
        (16, r.crtc.values.as_slice()),
        (48, r.graphics.values.as_slice()),
        (64, r.attribute.values.as_slice()),
    ] {
        bytes[offset..offset + bank.len()].copy_from_slice(bank);
    }
    for (slot, word) in bytes[96..116].as_chunks_mut::<4>().0.iter_mut().zip([
        u32::from(r.miscellaneous),
        u32::from(r.attribute.address()),
        u32::from(r.dac.mask),
        u32::from(snapshot.blink_on),
        geometry.scanline_step,
    ]) {
        slot.copy_from_slice(&word.to_le_bytes());
    }
    bytes
}
