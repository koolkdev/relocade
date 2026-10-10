use super::*;

#[test]
fn complete_ranges_register_without_a_snapshot_and_share_alias_invalidation() {
    let (mut cache, mut table, cpu) = fixture();
    let ticket = cache
        .register(
            &cpu,
            0x1000,
            &[
                CodeRange {
                    offset: 0x1000,
                    bytes: 1,
                },
                CodeRange {
                    offset: 0x3000,
                    bytes: 2,
                },
            ],
            &mut table,
        )
        .unwrap();
    assert!(cache.is_pending(ticket));
    assert!(watched(&table, 1) && watched(&table, 2) && watched(&table, 3));
    assert!(cache.install(ticket, &mut table));
    assert!(!cache.is_pending(ticket));
    assert!(!cache.install(ticket, &mut table));
    assert_eq!(cache.lookup(0x1000), Some(ticket));
    cache.invalidate_write(&mut table, 0x3001, 1);
    assert_eq!(cache.lookup(0x1000), None);
    assert!(!watched(&table, 1) && !watched(&table, 2) && !watched(&table, 3));
}

#[test]
fn incomplete_registration_keeps_existing_lifetimes_and_adds_no_watches() {
    let (mut cache, mut table, cpu) = fixture();
    let pending = cache.capture(&cpu, 0x1000, 1, &mut table).unwrap().ticket;
    let incomplete = [
        CodeRange {
            offset: 0x1000,
            bytes: 1,
        },
        CodeRange {
            offset: 0x3fff,
            bytes: 2,
        }, // Page 4 is absent.
    ];
    assert!(cache
        .register(&cpu, 0x1000, &incomplete, &mut table)
        .is_none());
    assert!(cache.is_pending(pending));
    assert!(!watched(&table, 3));
    assert!(cache.install(pending, &mut table));
    assert!(cache
        .register(&cpu, 0x1000, &incomplete, &mut table)
        .is_none());
    assert_eq!(cache.lookup(0x1000), Some(pending));
    assert!(cache.register(&cpu, 0x1000, &[], &mut table).is_none());
    assert!(cache
        .register(
            &cpu,
            0x1000,
            &[CodeRange {
                offset: 0x1000,
                bytes: 0
            }],
            &mut table
        )
        .is_none());
    assert!(cache
        .register(
            &cpu,
            0x1000,
            &[CodeRange {
                offset: 0x3000,
                bytes: 1
            }],
            &mut table
        )
        .is_none());
    assert_eq!(cache.lookup(0x1000), Some(pending));
}

#[test]
fn registration_requires_full_cs_coverage_while_capture_accepts_a_prefix() {
    let (_, mut table, mut cpu) = fixture();
    let mut cache = CodeCache::new(SegmentProfile::Segmented32.into(), &table);
    cpu.segments.cs.limit = 0x1000;
    assert!(cache
        .register(
            &cpu,
            0x1000,
            &[CodeRange {
                offset: 0x1000,
                bytes: 2
            }],
            &mut table
        )
        .is_none());
    assert!(!watched(&table, 1));
    let capture = cache.capture(&cpu, 0x1000, 1, &mut table).unwrap();
    assert_eq!(capture.copy_bytes(&vec![0x40; 0x4000]), [0x40]);
}

#[test]
fn wrapping_ranges_protect_both_sides_and_remaps_invalidate_them() {
    let (mut cache, mut table, cpu) = fixture();
    for page in [0, 0xfffff] {
        cache.remap(
            &mut table,
            page,
            Mapping::Ram {
                backing: 0x3000,
                writable: true,
            },
        );
    }
    let ticket = cache
        .register(
            &cpu,
            0xfffffff8,
            &[CodeRange {
                offset: 0xfffffff8,
                bytes: 16,
            }],
            &mut table,
        )
        .unwrap();
    assert!(watched(&table, 0) && watched(&table, 0xfffff) && watched(&table, 3));
    cache.remap(&mut table, 0, Mapping::Unmapped);
    assert!(!cache.install(ticket, &mut table));
    assert!(!watched(&table, 0xfffff));
}
