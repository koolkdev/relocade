//! Firmware command exchanges over the guest's shared controller ports.

use super::Error;
use futures_util::future::{select, Either};
use relocade_machine::{Clock, IoBus};
use relocade_pic::{CASCADE_IRQ, MASK_PORT, MOUSE_IRQ, SLAVE_MASK_PORT};
use relocade_ps2::protocol::{controller, mouse, ACK, RESEND};
use relocade_ps2::{read_auxiliary_byte, Controller, DATA_PORT, STATUS_COMMAND_PORT};
use std::{
    cell::{Cell, RefCell},
    future::Future,
    pin::pin,
    rc::Rc,
    time::Duration,
};

// BIOS owns the normal PC controller policy: translated keyboard with IRQ1.
// Auxiliary output is inhibited between commands while mouse reporting is off.
const COMMAND_REPLY: u8 =
    controller::KEYBOARD_IRQ_ENABLED | controller::SYSTEM_INITIALIZED | controller::TRANSLATE_SET_2;
const DISABLED: u8 = COMMAND_REPLY | controller::AUXILIARY_DISABLED;
const REPORTING: u8 = COMMAND_REPLY | controller::AUXILIARY_IRQ_ENABLED;

pub(super) struct Interface {
    clock: Clock,
    controller: Rc<Controller>,
    busy: Cell<bool>,
    enabled: Cell<bool>,
    packet: RefCell<Packet>,
}

impl Interface {
    pub fn new(clock: Clock, controller: Rc<Controller>) -> Self {
        Self {
            clock,
            controller,
            busy: Cell::new(false),
            enabled: Cell::new(false),
            packet: RefCell::new(Packet::default()),
        }
    }

    pub async fn command<const N: usize>(
        &self,
        io: &IoBus,
        bytes: &[u8],
    ) -> Result<[u8; N], Error> {
        if self.busy.replace(true) {
            return Err(Error::Interface);
        }
        let _busy = Busy(&self.busy);
        self.packet.borrow_mut().len = 0;
        let master_mask = io.read_port(MASK_PORT, 1);
        let mask = io.read_port(SLAVE_MASK_PORT, 1);
        // Mask the cascade first: withdrawing a slave request while master IRQ2
        // remains eligible would otherwise deliver a spurious IRQ15.
        io.write_port(MASK_PORT, 1, master_mask | (1 << CASCADE_IRQ));
        io.write_port(SLAVE_MASK_PORT, 1, mask | (1 << (MOUSE_IRQ - 8)));
        let result = self.exchange(io, bytes).await;
        if result.is_ok() {
            match bytes[0] {
                mouse::ENABLE_REPORTING => self.enabled.set(true),
                mouse::DISABLE_REPORTING | mouse::RESET => self.enabled.set(false),
                _ => {}
            }
        }
        // Complete controller restoration before returning to the guest. A canceled
        // session leaves the live machine alone; only the local busy flag has Drop.
        let restored = self
            .configure(
                io,
                if self.enabled.get() {
                    REPORTING
                } else {
                    DISABLED
                },
            )
            .await;
        // No await after unmasking: the busy guard is released before a pending
        // IRQ12 can run, including output published during controller restoration.
        io.write_port(SLAVE_MASK_PORT, 1, mask);
        io.write_port(MASK_PORT, 1, master_mask);
        restored?;
        result
    }

    /// Each hardware IRQ owns at most one unread auxiliary byte. Command replies
    /// belong to the exchange above; only enabled movement reaches a callback.
    pub fn packet(&self, io: &IoBus) -> Option<[u8; 3]> {
        if self.busy.get() {
            return None;
        }
        let byte = read_auxiliary_byte(io)?;
        if !self.enabled.get() {
            return None;
        }
        let mut packet = self.packet.borrow_mut();
        if packet.len == 0 && byte & mouse::PACKET_HEADER == 0 {
            return None;
        }
        let index = packet.len;
        packet.bytes[index] = byte;
        packet.len += 1;
        if packet.len == packet.bytes.len() {
            packet.len = 0;
            Some(packet.bytes)
        } else {
            None
        }
    }

    async fn exchange<const N: usize>(&self, io: &IoBus, bytes: &[u8]) -> Result<[u8; N], Error> {
        self.configure(io, DISABLED).await?;
        // Only a previously published mouse byte can remain. The new command
        // cancels unsent movement; keyboard bytes remain available to IRQ1.
        read_auxiliary_byte(io);
        self.send(io, bytes[0]).await?;
        self.configure(io, COMMAND_REPLY).await?;
        self.acknowledge(io).await?;
        for &parameter in &bytes[1..] {
            self.send(io, parameter).await?;
            self.acknowledge(io).await?;
        }
        let mut response = [0; N];
        for byte in &mut response {
            *byte = self.receive(io).await?;
        }
        Ok(response)
    }

    async fn acknowledge(&self, io: &IoBus) -> Result<(), Error> {
        match self.receive(io).await? {
            ACK => Ok(()),
            RESEND => Err(Error::Resend),
            _ => Err(Error::Interface),
        }
    }

    async fn send(&self, io: &IoBus, value: u8) -> Result<(), Error> {
        self.write(io, STATUS_COMMAND_PORT, controller::WRITE_AUXILIARY)
            .await?;
        self.write(io, DATA_PORT, value).await
    }

    async fn configure(&self, io: &IoBus, value: u8) -> Result<(), Error> {
        self.write(io, STATUS_COMMAND_PORT, controller::WRITE_COMMAND_BYTE)
            .await?;
        self.write(io, DATA_PORT, value).await
    }

    async fn write(&self, io: &IoBus, port: u16, value: u8) -> Result<(), Error> {
        self.wait(self.controller.wait_input_ready()).await?;
        io.write_port(port, 1, u32::from(value));
        self.wait(self.controller.wait_input_ready()).await
    }

    async fn wait<T>(&self, ready: impl Future<Output = T>) -> Result<T, Error> {
        let ready = pin!(ready);
        let timeout = pin!(self.clock.sleep(Duration::from_millis(100)));
        match select(ready, timeout).await {
            Either::Left((value, _)) => Ok(value),
            Either::Right(_) => Err(Error::Interface),
        }
    }

    async fn receive(&self, io: &IoBus) -> Result<u8, Error> {
        self.wait(self.controller.wait_auxiliary_output()).await?;
        // Read synchronously after readiness; no other task can consume the latch.
        Ok(read_auxiliary_byte(io).expect("auxiliary output is ready"))
    }
}

#[derive(Default)]
struct Packet {
    bytes: [u8; 3],
    len: usize,
}

struct Busy<'a>(&'a Cell<bool>);
impl Drop for Busy<'_> {
    fn drop(&mut self) {
        self.0.set(false);
    }
}
