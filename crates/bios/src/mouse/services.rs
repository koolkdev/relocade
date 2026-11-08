//! Pointing-device services declare their register ABI through the BIOS adapters.

use super::{Error, Mouse};
use relocade_machine::{
    real_mode_interrupt::{interrupt, interrupt_dispatch},
    Clock, CpuContext, FarPointer, IoBus,
};
use relocade_pic::{CASCADE_IRQ, MOUSE_IRQ};
use relocade_ps2::protocol::{mouse, SELF_TEST_PASSED};

impl<C: Clock> Mouse<C> {
    #[interrupt(context(call), args(function = ax))]
    pub(crate) async fn dispatch(&self, call: &mut CpuContext<'_>, function: u16) {
        interrupt_dispatch!(
            call,
            match function {
                0xc200 => self.set_enabled,
                0xc201 => self.reset,
                0xc202 => self.set_sample_rate,
                0xc203 => self.set_resolution,
                0xc204 => self.identify,
                0xc205 => self.initialize,
                0xc206 => self.extended,
                0xc207 => self.set_callback,
                0xc200..=0xc2ff => invalid_function,
                _ => crate::interrupts::unsupported,
            }
        );
    }

    #[interrupt(context(io), args(enabled = bh), returns {
        Ok(()) => { ah = 0, cf = false },
        Err(error) => { ah = error as u8, cf = true },
    })]
    async fn set_enabled(&self, io: &IoBus, enabled: u8) -> Result<(), Error> {
        match enabled {
            0 => {
                self.interface
                    .command::<0>(io, &[mouse::DISABLE_REPORTING])
                    .await?;
            }
            1 => {
                if self.callback.get().is_none() {
                    return Err(Error::NoHandler);
                }
                self.interface
                    .command::<0>(io, &[mouse::ENABLE_REPORTING])
                    .await?;
                // Enable both parts of the fixed PC mouse interrupt route.
                let mask = io.read_port(relocade_pic::SLAVE_MASK_PORT, 1);
                io.write_port(
                    relocade_pic::SLAVE_MASK_PORT,
                    1,
                    mask & !(1 << (MOUSE_IRQ - 8)),
                );
                let mask = io.read_port(relocade_pic::MASK_PORT, 1);
                io.write_port(relocade_pic::MASK_PORT, 1, mask & !(1 << CASCADE_IRQ));
            }
            _ => return Err(Error::InvalidParameter),
        };
        Ok(())
    }

    #[interrupt(context(io), returns {
        Ok([bat, id]) => { ah = 0, bl = bat, bh = id, cf = false },
        Err(error) => { ah = error as u8, cf = true },
    })]
    async fn reset(&self, io: &IoBus) -> Result<[u8; 2], Error> {
        let response = self.interface.command(io, &[mouse::RESET]).await?;
        if response[0] != SELF_TEST_PASSED {
            return Err(Error::Interface);
        }
        Ok(response)
    }

    #[interrupt(context(io), args(selection = bh), returns {
        Ok(()) => { ah = 0, cf = false },
        Err(error) => { ah = error as u8, cf = true },
    })]
    async fn set_sample_rate(&self, io: &IoBus, selection: u8) -> Result<(), Error> {
        let rate = *[10, 20, 40, 60, 80, 100, 200]
            .get(usize::from(selection))
            .ok_or(Error::InvalidParameter)?;
        self.interface
            .command::<0>(io, &[mouse::SET_SAMPLE_RATE, rate])
            .await?;
        Ok(())
    }

    #[interrupt(context(io), args(resolution = bh), returns {
        Ok(()) => { ah = 0, cf = false },
        Err(error) => { ah = error as u8, cf = true },
    })]
    async fn set_resolution(&self, io: &IoBus, resolution: u8) -> Result<(), Error> {
        if resolution > 3 {
            return Err(Error::InvalidParameter);
        }
        self.interface
            .command::<0>(io, &[mouse::SET_RESOLUTION, resolution])
            .await?;
        Ok(())
    }

    #[interrupt(context(io), returns {
        Ok([id]) => { ah = 0, bh = id, cf = false },
        Err(error) => { ah = error as u8, cf = true },
    })]
    async fn identify(&self, io: &IoBus) -> Result<[u8; 1], Error> {
        self.interface.command(io, &[mouse::IDENTIFY]).await
    }

    #[interrupt(context(io), args(packet_size = bh), returns {
        Ok(()) => { ah = 0, cf = false },
        Err(error) => { ah = error as u8, cf = true },
    })]
    async fn initialize(&self, io: &IoBus, packet_size: u8) -> Result<(), Error> {
        if packet_size != 3 {
            return Err(if (1..=8).contains(&packet_size) {
                Error::Interface
            } else {
                Error::InvalidParameter
            });
        }
        self.reset(io).await?;
        self.callback.set(None);
        Ok(())
    }

    #[interrupt(context(call), args(function = bh))]
    async fn extended(&self, call: &mut CpuContext<'_>, function: u8) {
        interrupt_dispatch!(
            call,
            match function {
                0 => self.status,
                1 => self.set_scaling(false),
                2 => self.set_scaling(true),
                _ => invalid_parameter,
            }
        );
    }

    #[interrupt(context(io), returns {
        Ok([status, resolution, rate]) => {
            ah = 0, bl = status, cl = resolution, dl = rate, cf = false
        },
        Err(error) => { ah = error as u8, cf = true },
    })]
    async fn status(&self, io: &IoBus) -> Result<[u8; 3], Error> {
        self.interface.command(io, &[mouse::STATUS]).await
    }

    #[interrupt(context(io), returns {
        Ok(()) => { ah = 0, cf = false },
        Err(error) => { ah = error as u8, cf = true },
    })]
    async fn set_scaling(&self, io: &IoBus, doubled: bool) -> Result<(), Error> {
        let command = if doubled {
            mouse::SCALING_2_TO_1
        } else {
            mouse::SCALING_1_TO_1
        };
        self.interface.command::<0>(io, &[command]).await?;
        Ok(())
    }

    #[interrupt(context(io), args(target = ptr(es, bx)), returns {
        Ok(()) => { ah = 0, cf = false },
        Err(error) => { ah = error as u8, cf = true },
    })]
    async fn set_callback(&self, io: &IoBus, target: FarPointer) -> Result<(), Error> {
        if target.packed() == 0 {
            self.interface
                .command::<0>(io, &[mouse::DISABLE_REPORTING])
                .await?;
            self.callback.set(None);
        } else {
            self.callback.set(Some(target));
        }
        Ok(())
    }
}

#[interrupt(returns { () => { ah = Error::InvalidFunction as u8, cf = true } })]
fn invalid_function() {}

#[interrupt(returns { () => { ah = Error::InvalidParameter as u8, cf = true } })]
fn invalid_parameter() {}
