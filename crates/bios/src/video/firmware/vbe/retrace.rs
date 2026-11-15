//! Shared wait and revalidation for services synchronized to vertical retrace.
use super::Vbe;
use relocade_vga::RetraceEvent;
use std::cell::RefCell;

pub(super) async fn wait<C: Vbe>(card: &RefCell<C>, valid: impl Fn(&C) -> bool) -> Option<()> {
    loop {
        let wait = card.borrow_mut().wait_for_retrace()?;
        let event = wait.await;
        // No card borrow spans suspension. Configuration wakes must revalidate
        // even when the new mode no longer produces retrace pulses.
        if !valid(&*card.borrow()) {
            return None;
        }
        if event == RetraceEvent::Started {
            return Some(());
        }
    }
}
