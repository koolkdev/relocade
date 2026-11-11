//! Programming effects bracket register mutation. Their concrete callbacks are
//! prepared once and selected when the register index or display mode changes.

use crate::{SvgaLayout, Timing, Vga};
use relocade_machine::{BytePort, DeviceCallbacks, IoContext, PortWrite};

/// Required work around a register mutation, resolved at configuration boundaries.
#[derive(Clone, Copy)]
pub(crate) enum WriteEffect {
    RegistersOnly,
    Display,
    Timing,
}

/// Cached operations for the three programming effects. The port map owns the
/// active choice; this owner retains definitions only.
pub(crate) struct RegisterWrites {
    pub registers: PortWrite,
    pub display: PortWrite,
    pub timing: PortWrite,
}

impl RegisterWrites {
    pub fn new(
        data: &BytePort,
        callbacks: &DeviceCallbacks<Vga>,
        write: impl Fn(IoContext<'_>, &mut Vga, u8) + Clone + 'static,
    ) -> Self {
        Self {
            registers: data.write_handler(callbacks.write(write.clone())),
            display: data.write_handler(callbacks.write(display_write(write.clone()))),
            timing: data.write_handler(callbacks.write(timing_write(write))),
        }
    }

    pub fn get(&self, effect: WriteEffect) -> &PortWrite {
        match effect {
            WriteEffect::RegistersOnly => &self.registers,
            WriteEffect::Display => &self.display,
            WriteEffect::Timing => &self.timing,
        }
    }
}

pub(crate) fn display_write<T: 'static>(
    write: impl Fn(IoContext<'_>, &mut Vga, T) + 'static,
) -> impl Fn(IoContext<'_>, &mut Vga, T) + 'static {
    move |context, vga, value| {
        vga.capture_if_due();
        write(context, vga, value);
    }
}

fn timing_write<T: 'static>(
    write: impl Fn(IoContext<'_>, &mut Vga, T) + 'static,
) -> impl Fn(IoContext<'_>, &mut Vga, T) + 'static {
    move |mut context, vga, value| {
        vga.advance_raster(vga.clock.now());
        let previous_layout = vga.dispi.state.display_layout();
        write(context.reborrow(), vga, value);
        vga.configure_display(context, previous_layout);
    }
}

impl Vga {
    pub(super) fn configure_display(
        &mut self,
        context: IoContext<'_>,
        previous_layout: Option<SvgaLayout>,
    ) {
        let layout = self.dispi.state.display_layout();
        let timing = match layout {
            Some(layout) => Timing::svga(layout),
            None => Timing::vga(&self.registers),
        };
        let timing_changed = if layout.map(|layout| layout.mode().resolution())
            != previous_layout.map(|layout| layout.mode().resolution())
        {
            self.raster.restart(timing);
            true
        } else {
            self.raster.set_timing(timing)
        };
        self.configure_ports(context);
        if timing_changed || layout != previous_layout {
            self.programming_changed.notify(usize::MAX);
        }
    }
}
