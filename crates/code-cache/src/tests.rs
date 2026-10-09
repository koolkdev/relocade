use super::*;
use wasm86_x86::{SegmentProfile, Segments, CODE_WATCH};

mod registration;

fn fixture() -> (CodeCache, Vec<u8>, CpuState) {
    let mut table = vec![0; 4 << 20];
    for (page, backing) in [(1, 0x2000u32), (2, 0x2000), (3, 0x3000)] {
        table[page * 4..page * 4 + 4].copy_from_slice(&(backing | 3).to_le_bytes());
    }
    let cache = CodeCache::new(SegmentProfile::Flat32.into(), &table);
    (cache, table, CpuState::default())
}
fn watched(table: &[u8], page: usize) -> bool {
    u32::from_le_bytes(table[page * 4..page * 4 + 4].try_into().unwrap()) & CODE_WATCH != 0
}

#[test]
fn pending_watches_cover_aliases_and_reject_late_results() {
    let (mut cache, mut table, cpu) = fixture();
    let captured = cache.capture(&cpu, 0x1000, 1, &mut table).unwrap();
    assert!(watched(&table, 1) && watched(&table, 2));
    let mut backing = vec![0; 0x4000];
    backing[0x2000..0x200f].fill(0x90);
    assert_eq!(captured.copy_bytes(&backing), vec![0x90; 15]);
    cache.invalidate_write(&mut table, 0x2007, 1);
    assert!(!cache.install(captured.ticket, &mut table));
    assert_eq!(cache.lookup(0x1000), None);
    assert!(!watched(&table, 1) && !watched(&table, 2));
}

#[test]
fn independent_tickets_share_watches_until_the_last_release() {
    let (mut cache, mut table, cpu) = fixture();
    let first = cache.capture(&cpu, 0x1000, 1, &mut table).unwrap().ticket;
    let second = cache.capture(&cpu, 0x2000, 1, &mut table).unwrap().ticket;
    assert!(cache.install(first, &mut table));
    cache.cancel(second, &mut table);
    assert!(watched(&table, 2));
    assert_eq!(cache.lookup(0x1000), Some(first));
    cache.invalidate_backing(&mut table, 0x2fff, 1);
    assert_eq!(cache.lookup(0x1000), None);
    assert!(!watched(&table, 2));
}

#[test]
fn remapping_aba_cannot_revive_a_capture_and_new_aliases_inherit_watches() {
    let (mut cache, mut table, cpu) = fixture();
    let first = cache.capture(&cpu, 0x1000, 1, &mut table).unwrap().ticket;
    cache.remap(
        &mut table,
        4,
        Mapping::Ram {
            backing: 0x2000,
            writable: true,
        },
    );
    assert!(watched(&table, 4));
    cache.remap(
        &mut table,
        1,
        Mapping::Ram {
            backing: 0x3000,
            writable: true,
        },
    );
    cache.remap(
        &mut table,
        1,
        Mapping::Ram {
            backing: 0x2000,
            writable: true,
        },
    );
    assert!(!cache.install(first, &mut table));
    let second = cache.capture(&cpu, 0x1000, 1, &mut table).unwrap().ticket;
    assert_ne!(first, second);
    cache.invalidate_write(&mut table, 0x4000, 1);
    assert!(!cache.install(second, &mut table));
}

#[test]
fn changing_cs_invalidates_installed_and_pending_contexts() {
    let (_, mut table, mut cpu) = fixture();
    // Segmented profiles admit two different valid code segments.
    let mut cache = CodeCache::new(SegmentProfile::Segmented32.into(), &table);
    let first = cache.capture(&cpu, 0x1000, 1, &mut table).unwrap().ticket;
    assert!(cache.install(first, &mut table));
    let second = cache.capture(&cpu, 0x2000, 1, &mut table).unwrap().ticket;
    cpu.segments.cs.base = 0x1000;
    assert!(cache.enter(&cpu, &mut table));
    assert_eq!(cache.lookup(0x1000), None);
    assert!(!cache.install(second, &mut table));
    cpu.segments.cs.base = 0;
    assert!(cache.enter(&cpu, &mut table));
    assert!(!cache.install(first, &mut table));
}

#[test]
fn snapshots_stop_at_fetch_limits_and_unmapped_pages() {
    let (mut cache, mut table, mut cpu) = fixture();
    let capture = cache.capture(&cpu, 0x3ffc, 2, &mut table).unwrap();
    assert_eq!(capture.copy_bytes(&vec![0x90; 0x4000]), [0x90; 4]);
    cache.clear(&mut table);
    cache = CodeCache::new(SegmentProfile::Segmented32.into(), &table);
    cpu.segments.cs.limit = 0x1001;
    let capture = cache.capture(&cpu, 0x1000, 2, &mut table).unwrap();
    assert_eq!(capture.copy_bytes(&vec![0x90; 0x4000]), [0x90; 2]);
    assert!(cache.capture(&cpu, 0x1002, 1, &mut table).is_none());
}

#[test]
fn real_mode_rom_code_protects_writable_aliases() {
    let mut table = vec![0; 65536];
    table[8..16].copy_from_slice(&[2, 0, 0, 0, 0, 0x30, 0, 0]);
    table[16..24].copy_from_slice(&[1, 0, 0, 0, 0, 0x30, 0, 0]);
    let mut cache = CodeCache::new(ExecutionProfile::Real16, &table);
    let cpu = CpuState {
        segments: Segments::real_mode(),
        ..CpuState::default()
    };
    let ticket = cache.capture(&cpu, 0x1000, 1, &mut table).unwrap().ticket;
    assert_eq!(table[8], 2 | CODE_WATCH as u8);
    assert_eq!(table[16], 1 | CODE_WATCH as u8);
    cache.invalidate_write(&mut table, 0x2000, 1);
    assert!(!cache.install(ticket, &mut table));
    assert_eq!(table[8], 2);
    assert_eq!(table[16], 1);
}

#[test]
fn replacement_keeps_installed_code_until_accepted() {
    let (mut cache, mut table, cpu) = fixture();
    let installed = cache.capture(&cpu, 0x1000, 1, &mut table).unwrap().ticket;
    assert!(cache.install(installed, &mut table));
    let failed = cache.capture(&cpu, 0x1000, 2, &mut table).unwrap().ticket;
    assert_eq!(cache.lookup(0x1000), Some(installed));
    cache.cancel(failed, &mut table);
    assert_eq!(cache.lookup(0x1000), Some(installed));
    let replacement = cache.capture(&cpu, 0x1000, 2, &mut table).unwrap().ticket;
    assert!(cache.install(replacement, &mut table));
    assert_eq!(cache.lookup(0x1000), Some(replacement));
    assert!(!cache.contains(installed));
    assert!(watched(&table, 2));
}
