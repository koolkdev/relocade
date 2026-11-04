//! INT 10h string formats and cursor-update policy.
use crate::{screen::PageWriter, Teletype};
use relocade_machine::{
    real_mode_interrupt::{interrupt, FarPointer},
    Clock,
};

pub(crate) struct StringRun {
    page: u8,
    /// Starting row in the high byte and column in the low byte.
    position: u16,
    source: FarPointer,
    count: u16,
}

impl<C: Clock> Teletype<C> {
    /// Writes characters with one attribute, preserving the page's stored cursor.
    #[interrupt(
    args(run = StringRun { page: bh, position: dx, source: ptr(es, bp), count: cx }, attribute = bl),
    returns { Some(()) => {}, None => crate::interrupt::unsupported },
)]
    pub(crate) async fn write_string(&self, run: StringRun, attribute: u8) -> Option<()> {
        self.write(run, Some(attribute), false).await
    }

    /// Writes characters with one attribute and moves the page's stored cursor.
    #[interrupt(
    args(run = StringRun { page: bh, position: dx, source: ptr(es, bp), count: cx }, attribute = bl),
    returns { Some(()) => {}, None => crate::interrupt::unsupported },
)]
    pub(crate) async fn write_string_and_move_cursor(
        &self,
        run: StringRun,
        attribute: u8,
    ) -> Option<()> {
        self.write(run, Some(attribute), true).await
    }

    /// Writes character/attribute pairs, preserving the page's stored cursor.
    #[interrupt(
    args(run = StringRun { page: bh, position: dx, source: ptr(es, bp), count: cx }),
    returns { Some(()) => {}, None => crate::interrupt::unsupported },
)]
    pub(crate) async fn write_cells(&self, run: StringRun) -> Option<()> {
        self.write(run, None, false).await
    }

    /// Writes character/attribute pairs and moves the page's stored cursor.
    #[interrupt(
    args(run = StringRun { page: bh, position: dx, source: ptr(es, bp), count: cx }),
    returns { Some(()) => {}, None => crate::interrupt::unsupported },
)]
    pub(crate) async fn write_cells_and_move_cursor(&self, run: StringRun) -> Option<()> {
        self.write(run, None, true).await
    }

    async fn write(&self, run: StringRun, attribute: Option<u8>, move_cursor: bool) -> Option<()> {
        if run.count == 0 {
            return Some(());
        }
        let mut memory = self.memory.view();
        let mut writer = PageWriter::new(&mut memory, self.memory.io(), run.page)?;
        writer.set_position(run.position);
        let bytes_per_character = if attribute.is_some() { 1 } else { 2 };
        let mut source = run.source;
        for _ in 0..run.count {
            let mut cell = [0; 2];
            // Read each cell live: the source may alias the page being written.
            self.memory
                .view()
                .read_wrapping(source, &mut cell[..bytes_per_character])
                .expect("string character");
            source = source.wrapping_add(bytes_per_character);
            self.write_to_page(&mut writer, cell[0], attribute.unwrap_or(cell[1]), true)
                .await;
        }
        if move_cursor {
            writer.commit_cursor();
        }
        Some(())
    }
}
