//! Shared GPU bindings for mode-specific settings, physical VRAM and the DAC.
use crate::{DisplaySnapshot, PixelFormat, Scanout};

// Settings fit the VGA banks and controls; packed scanout uses only three words.
const SETTINGS_BYTES: usize = 128;
// WGSL aligns the palette's vec4 array after the DAC mask to 16 bytes.
const PALETTE_OFFSET: usize = 16;
const PALETTE_BYTES: usize = 256 * 3;

pub(super) struct GpuState {
    settings: wgpu::Buffer,
    dac: wgpu::Buffer,
    previous_settings: Option<[u8; SETTINGS_BYTES]>,
    previous_palette: Option<[[u8; 3]; 256]>,
    previous_mask: Option<u8>,
    pub layout: wgpu::BindGroupLayout,
    pub binding: wgpu::BindGroup,
}

impl GpuState {
    pub fn new(device: &wgpu::Device, memory: &wgpu::Buffer) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("captured display inputs"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(SETTINGS_BYTES as u64),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(4),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(
                            (PALETTE_OFFSET + PALETTE_BYTES) as u64,
                        ),
                    },
                    count: None,
                },
            ],
        });
        let settings = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("captured display settings"),
            size: SETTINGS_BYTES as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let dac = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("captured DAC"),
            size: (PALETTE_OFFSET + PALETTE_BYTES) as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let binding = bind(device, &layout, &settings, memory, &dac);
        Self {
            settings,
            dac,
            previous_settings: None,
            previous_palette: None,
            previous_mask: None,
            layout,
            binding,
        }
    }

    pub fn rebind_memory(&mut self, device: &wgpu::Device, memory: &wgpu::Buffer) {
        self.binding = bind(device, &self.layout, &self.settings, memory, &self.dac);
    }

    pub fn upload(&mut self, queue: &wgpu::Queue, snapshot: &DisplaySnapshot) -> bool {
        let settings = settings(snapshot);
        let settings_changed = self.previous_settings.as_ref() != Some(&settings);
        if settings_changed {
            queue.write_buffer(&self.settings, 0, &settings);
            self.previous_settings = Some(settings);
        }
        if matches!(snapshot.scanout(), Scanout::Packed(layout)
            if layout.format() != PixelFormat::Indexed8)
        {
            return settings_changed;
        }
        let mask = snapshot.dac.mask;
        let mask_changed = self.previous_mask != Some(mask);
        if mask_changed {
            queue.write_buffer(&self.dac, 0, &u32::from(mask).to_le_bytes());
            self.previous_mask = Some(mask);
        }
        let palette = &snapshot.dac.colors;
        let palette_changed = self.previous_palette.as_ref() != Some(palette);
        if palette_changed {
            queue.write_buffer(&self.dac, PALETTE_OFFSET as u64, palette.as_flattened());
            self.previous_palette = Some(*palette);
        }
        settings_changed || mask_changed || palette_changed
    }
}

fn bind(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    settings: &wgpu::Buffer,
    memory: &wgpu::Buffer,
    dac: &wgpu::Buffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("captured display inputs"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: settings.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: memory.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: dac.as_entire_binding(),
            },
        ],
    })
}

fn settings(snapshot: &DisplaySnapshot) -> [u8; SETTINGS_BYTES] {
    let mut bytes = [0; SETTINGS_BYTES];
    match snapshot.scanout() {
        Scanout::Vga(r) => {
            for (offset, bank) in [
                (0, r.sequencer.as_slice()),
                (16, r.crtc.values.as_slice()),
                (48, r.graphics.as_slice()),
                (64, r.attributes.as_slice()),
            ] {
                bytes[offset..offset + bank.len()].copy_from_slice(bank);
            }
            for (slot, word) in bytes[96..124].as_chunks_mut::<4>().0.iter_mut().zip([
                u32::from(r.miscellaneous),
                u32::from(r.display_enabled),
                u32::from(r.attribute_blink_on),
                u32::from(r.cursor_blink_on),
                r.geometry().scanline_step,
                u32::from(r.display_start),
                u32::from(r.linear_addressing),
            ]) {
                slot.copy_from_slice(&word.to_le_bytes());
            }
        }
        Scanout::Packed(layout) => {
            bytes[..4].copy_from_slice(&layout.pitch().to_le_bytes());
            bytes[4..8].copy_from_slice(&layout.display_start().to_le_bytes());
            // Decoder tags shared with packed.wgsl; separate from storage width.
            let format: u32 = match layout.format() {
                PixelFormat::Indexed8 => 0,
                PixelFormat::Rgb565 => 1,
                PixelFormat::Bgrx8888 => 2,
            };
            bytes[8..12].copy_from_slice(&format.to_le_bytes());
        }
    }
    bytes
}
