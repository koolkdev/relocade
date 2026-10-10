//! Host-owned code lifetime and page write protection.
//!
//! One owner covers a guest backing memory and its mapping table. Registration,
//! capture and installation run between Wasm invocations. Guest callbacks may
//! invalidate existing code, but must not add watches while generated access
//! proofs are live.
//! Compilation workers receive only owned bytes and an opaque ticket.

#![forbid(unsafe_code)]

mod mapping;
mod registration;
mod snapshot;
#[cfg(test)]
mod tests;

pub use mapping::Mapping;
use mapping::Mappings;
pub use registration::CodeRange;
pub use snapshot::Capture;
use std::collections::{HashMap, HashSet};
use wasm86_x86::{CpuState, ExecutionProfile, StoredSegment};

/// A protected code lifetime, local to one owner. It need not have a compilation
/// request. Never pass tickets between owners or reuse one for different code:
/// delayed modules can outlive arbitrary writes/remaps.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Ticket(u64);
impl Ticket {
    pub fn id(self) -> u64 {
        self.0
    }
}

struct Entry {
    eip: u32,
    mappings: HashSet<u32>,
    backing: HashSet<u32>,
}

/// Owns validity and mapping watches; engine modules remain with the runtime.
/// Eager invalidation covers pending and installed tickets equally, so dispatch
/// needs no code scan or page-generation lookup. Mapping changes invalidate even
/// an A→B→A sequence while a worker still holds the original bytes.
pub struct CodeCache {
    profile: ExecutionProfile,
    context: Option<StoredSegment>,
    mappings: Mappings,
    next_ticket: u64,
    entries: HashMap<Ticket, Entry>,
    by_eip: HashMap<u32, Ticket>,
    pending: HashMap<u32, Ticket>,
    by_mapping: HashMap<u32, HashSet<Ticket>>,
    by_backing: HashMap<u32, HashSet<Ticket>>,
}

impl CodeCache {
    /// Adopts initialized, unwatched mapping metadata. Further writes and remaps
    /// must go through this owner; arbitrary raw table edits break its invariants.
    pub fn new(profile: ExecutionProfile, table: &[u8]) -> Self {
        Self {
            profile,
            context: None,
            mappings: Mappings::new(profile, table),
            next_ticket: 1,
            entries: HashMap::new(),
            by_eip: HashMap::new(),
            pending: HashMap::new(),
            by_mapping: HashMap::new(),
            by_backing: HashMap::new(),
        }
    }

    pub fn profile(&self) -> ExecutionProfile {
        self.profile
    }

    /// Admits the CPU context at an execution boundary. A changed CS invalidates
    /// code before selecting an entry, even if the profile itself still fits.
    pub fn enter(&mut self, cpu: &CpuState, table: &mut [u8]) -> bool {
        let compatible = self.profile.is_compatible_with(&cpu.segments);
        if !compatible || self.context != Some(cpu.segments.cs) {
            self.clear(table);
            self.context = compatible.then_some(cpu.segments.cs);
        }
        compatible
    }

    pub fn lookup(&self, eip: u32) -> Option<Ticket> {
        self.by_eip.get(&eip).copied()
    }

    pub fn contains(&self, ticket: Ticket) -> bool {
        self.entries.contains_key(&ticket)
    }

    /// Whether this registration still awaits installation. An installed ticket
    /// remains valid, but cannot be used to install a second module.
    pub fn is_pending(&self, ticket: Ticket) -> bool {
        self.entries
            .get(&ticket)
            .is_some_and(|entry| self.pending.get(&entry.eip) == Some(&ticket))
    }

    /// Accepts a completed module only while its registered dependencies remain live.
    pub fn install(&mut self, ticket: Ticket, table: &mut [u8]) -> bool {
        let Some(entry) = self.entries.get(&ticket) else {
            return false;
        };
        let eip = entry.eip;
        if self.pending.get(&eip) != Some(&ticket) {
            return false;
        }
        if let Some(previous) = self.by_eip.get(&eip).copied() {
            self.cancel(previous, table);
        }
        self.pending.remove(&eip);
        self.by_eip.insert(eip, ticket);
        true
    }

    /// Compilation failure/cancellation releases this ticket's watches without
    /// unprotecting backing still referenced by another pending/installed entry.
    pub fn cancel(&mut self, ticket: Ticket, table: &mut [u8]) {
        let Some(entry) = self.entries.remove(&ticket) else {
            return;
        };
        if self.by_eip.get(&entry.eip) == Some(&ticket) {
            self.by_eip.remove(&entry.eip);
        }
        if self.pending.get(&entry.eip) == Some(&ticket) {
            self.pending.remove(&entry.eip);
        }
        for page in entry.mappings {
            remove_dependency(&mut self.by_mapping, page, ticket);
        }
        for backing in entry.backing {
            if remove_dependency(&mut self.by_backing, backing, ticket) {
                self.mappings.watch_aliases(table, backing, false);
            }
        }
    }

    pub fn clear(&mut self, table: &mut [u8]) {
        for &backing in self.by_backing.keys() {
            self.mappings.watch_aliases(table, backing, false);
        }
        self.entries.clear();
        self.by_eip.clear();
        self.pending.clear();
        self.by_mapping.clear();
        self.by_backing.clear();
    }

    /// Invalidates every alias before a host backing write. Call before changing
    /// bytes, including device/DMA writes; guest stores use `invalidate_write`.
    pub fn invalidate_backing(&mut self, table: &mut [u8], start: u32, bytes: u32) {
        for page in pages(start, bytes) {
            let tickets = self
                .by_backing
                .get(&(page << 12))
                .cloned()
                .unwrap_or_default();
            for ticket in tickets {
                self.cancel(ticket, table);
            }
        }
    }

    /// The generated interpreter calls this just before an actual watched write.
    /// Virtual spans can wrap or scatter; physical callbacks pass RAM portions.
    /// Notification changes only coherence metadata, never architectural mappings.
    pub fn invalidate_write(&mut self, table: &mut [u8], start: u32, bytes: u32) {
        let backing: Vec<_> = pages(start, bytes)
            .filter_map(|page| self.mappings.get(page).backing())
            .collect();
        for backing in backing {
            self.invalidate_backing(table, backing, 4096);
        }
    }

    /// Replaces one page, invalidating dependencies on that mapping before exposing
    /// new routing. A new alias inherits existing backing watches immediately.
    pub fn remap(&mut self, table: &mut [u8], page: u32, mapping: Mapping) {
        let tickets = self.by_mapping.get(&page).cloned().unwrap_or_default();
        for ticket in tickets {
            self.cancel(ticket, table);
        }
        self.mappings.replace(page, mapping);
        let watched = mapping
            .backing()
            .is_some_and(|backing| self.by_backing.contains_key(&backing));
        self.mappings.write(table, page, watched);
    }
}

fn remove_dependency(map: &mut HashMap<u32, HashSet<Ticket>>, page: u32, ticket: Ticket) -> bool {
    let tickets = map.get_mut(&page).expect("live dependency");
    tickets.remove(&ticket);
    if tickets.is_empty() {
        map.remove(&page);
        true
    } else {
        false
    }
}

fn pages(start: u32, bytes: u32) -> impl Iterator<Item = u32> {
    let count = if bytes == 0 {
        0
    } else {
        ((u64::from(start & 4095) + u64::from(bytes) - 1) >> 12) + 1
    };
    (0..count).map(move |offset| ((start >> 12).wrapping_add(offset as u32)) & 0xfffff)
}
