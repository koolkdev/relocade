//! Text-specific memory selection and uniforms. Physical uploads live in `memory`.
use super::{
    memory::{GpuMemory, MemorySelection},
    upload::InputBuffer,
};
use crate::{DisplaySnapshot, TextDisplay, TextLayout};

const SETTINGS_WORDS: usize = 40;
const FONT_BANK_BYTES: usize = 8192;

pub(super) struct TextInput {
    memory: GpuMemory,
    selection: TextSelection,
    settings: InputBuffer,
    pub binding: wgpu::BindGroup,
}

impl TextInput {
    pub fn new(device: &wgpu::Device, layout: &wgpu::BindGroupLayout) -> Self {
        let memory = GpuMemory::new(device);
        let settings = InputBuffer::new(device, SETTINGS_WORDS * 4, wgpu::BufferUsages::UNIFORM);
        let binding = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("captured VGA text resources"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: settings.buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&memory.view),
                },
            ],
        });
        Self {
            memory,
            selection: TextSelection::new(),
            settings,
            binding,
        }
    }

    pub fn upload(&mut self, queue: &wgpu::Queue, snapshot: &DisplaySnapshot, text: &TextDisplay) {
        self.settings.update(queue, settings(text).as_flattened());
        let changed = self.selection.update(text);
        self.memory
            .upload(queue, snapshot, &self.selection.required, changed);
    }
}

struct TextSelection {
    layout: Option<TextLayout>,
    font_banks: [u16; 2],
    blanked: bool,
    required: MemorySelection,
}

impl TextSelection {
    fn new() -> Self {
        Self {
            layout: None,
            font_banks: [0; 2],
            blanked: false,
            required: MemorySelection::new(),
        }
    }

    fn update(&mut self, text: &TextDisplay) -> bool {
        let fonts = [
            text.font_b,
            if text.dual_font {
                text.font_a
            } else {
                text.font_b
            },
        ];
        if self.layout.as_ref() == Some(&text.layout)
            && self.font_banks == fonts
            && self.blanked == text.blanked
        {
            return false;
        }
        self.layout = Some(text.layout.clone());
        self.font_banks = fonts;
        self.blanked = text.blanked;
        self.required.clear();
        if text.blanked {
            return true;
        }

        let layout = &text.layout;
        for y in 0..layout.size[1] {
            let line = layout.scanline(y);
            for column in 0..(layout.size[0] + line.pan).div_ceil(layout.glyph_width) {
                let counter = line
                    .counter
                    .wrapping_add((column / layout.count_divisor) as u16);
                let address = usize::from(layout.address(counter, line.glyph_row));
                self.required.include_byte(0, address);
                self.required.include_byte(1, address);
            }
        }
        // Whole selected banks stay resident. Changing a character never needs
        // glyph discovery or font uploads; only font writes or bank selection do.
        for bank in fonts {
            let start = usize::from(bank);
            self.required
                .include_range(2, start..start + FONT_BANK_BYTES);
        }
        true
    }
}

fn settings(text: &TextDisplay) -> [[u8; 4]; SETTINGS_WORDS] {
    let mut words = [0u32; SETTINGS_WORDS];
    words[..22].copy_from_slice(&[
        text.layout.columns,
        text.layout.glyph_width,
        text.layout.glyph_height,
        u32::from(text.layout.start_address),
        u32::from(text.layout.stride),
        text.layout.preset_row,
        text.layout.byte_panning,
        text.layout.pel_panning,
        text.layout.line_compare,
        text.layout.scan_repeat,
        u32::from(text.layout.address_mode),
        u32::from(text.layout.underline),
        text.layout.count_divisor,
        u32::from(text.font_a),
        u32::from(text.font_b),
        u32::from(text.dual_font),
        u32::from(text.cursor_address),
        u32::from(text.cursor_start),
        u32::from(text.cursor_end),
        u32::from(text.attribute_mode),
        u32::from(text.blink_on),
        u32::from(text.blanked),
    ]);
    words[22] = text.layout.output_step;
    words[24..].copy_from_slice(&text.colors);
    words.map(u32::to_le_bytes)
}

#[cfg(test)]
mod tests;
