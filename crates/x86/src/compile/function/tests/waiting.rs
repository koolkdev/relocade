use super::*;
use futures_util::{future::poll_fn, FutureExt};
use std::{cell::Cell, pin::pin, task::Poll};

#[test]
fn awaited_reads_preserve_decoded_paths_and_remaining_limits() {
    let compiler = Compiler::new(SegmentProfile::Flat32);
    let prefix = [
        0x0f, 0x85, 0xfa, 0x1f, 0, 0, // JNE 3000
        0xe9, 0xf5, 0x0f, 0, 0, // JMP 2000
        0x90, 0x90, 0x90, 0x90,
    ];
    let available = Cell::new(0);
    let mut discovery = pin!(compiler.discover_function(
        CodeAddress::CsRelative(0x1000),
        FunctionLimits {
            instructions: 4,
            bytes: 13,
            ..FunctionLimits::default()
        },
        async |eip, output: &mut [u8]| {
            if matches!(eip, 0x2000 | 0x3000) {
                let required = if eip == 0x3000 { 1 } else { 2 };
                poll_fn(|_| {
                    if available.get() >= required {
                        Poll::Ready(())
                    } else {
                        Poll::Pending
                    }
                })
                .await;
                output[0] = 0xc3;
                1
            } else {
                reader(0x1000, &prefix)(eip, output)
            }
        },
    ));
    assert!(discovery.as_mut().now_or_never().is_none());
    available.set(1);
    assert!(discovery.as_mut().now_or_never().is_none());
    available.set(2);
    let function = discovery.as_mut().now_or_never().unwrap().unwrap();
    let mut module = compiler.function_module();
    let entries = module.add_discovered_function(function).unwrap();
    assert_eq!(
        entries
            .iter()
            .map(|entry| (entry.start_eip, entry.byte_len))
            .collect::<Vec<_>>(),
        [(0x1000, 6), (0x1006, 5), (0x2000, 1), (0x3000, 1)]
    );
    assert_eq!(module.compile().unwrap().entries.len(), 4);
}

#[test]
fn awaited_read_preserves_a_partial_instruction_across_a_page_boundary() {
    let compiler = Compiler::new(ExecutionProfile::Real16);
    let available = Cell::new(false);
    let prefix_reads = Cell::new(0);
    let mut discovery = pin!(compiler.discover_function(
        CodeAddress::CsRelative(0xffe),
        FunctionLimits::default(),
        async |eip, output: &mut [u8]| match eip {
            0xffe => {
                prefix_reads.set(prefix_reads.get() + 1);
                output[..2].copy_from_slice(&[0xb8, 0x34]); // MOV AX,1234 prefix
                2
            }
            0x1000 => {
                poll_fn(|_| {
                    if available.get() {
                        Poll::Ready(())
                    } else {
                        Poll::Pending
                    }
                })
                .await;
                output[..2].copy_from_slice(&[0x12, 0xc3]); // immediate suffix; RET
                2
            }
            0x1002 => 0,
            _ => panic!("unexpected read at {eip:x}"),
        },
    ));
    assert!(discovery.as_mut().now_or_never().is_none());
    available.set(true);
    let function = discovery.as_mut().now_or_never().unwrap().unwrap();
    assert_eq!(prefix_reads.get(), 1);
    let mut module = compiler.function_module();
    let entries = module.add_discovered_function(function).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!((entries[0].start_eip, entries[0].byte_len), (0xffe, 4));
}
