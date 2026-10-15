use crate::{Dos, DosStop};
use wasm86_machine::{Environment, HostAction, HostContext};
use wasm86_x86::{CpuState, Segment, StoredSegment};

impl Environment for Dos {}

pub(super) fn terminate(host: &mut HostContext<'_, Dos>) -> HostAction {
    stop(host, DosStop::Exited(0))
}

pub(super) fn int21(host: &mut HostContext<'_, Dos>) -> HostAction {
    let mut cpu = host.cpu();
    let function = (cpu.registers.eax >> 8) as u8;
    match function {
        0x00 => return terminate(host),
        0x02 => {
            let byte = cpu.registers.edx as u8;
            host.with_environment(|dos| dos.output.push(byte));
            let returned = if byte == b'\t' { b' ' } else { byte };
            cpu.registers.eax = (cpu.registers.eax & !0xff) | u32::from(returned);
        }
        0x09 => {
            let segment = cpu.segments.ds.selector;
            let start = cpu.registers.edx as u16;
            let mut output = Vec::new();
            for count in 0..=u16::MAX {
                let mut byte = [0];
                read_segment(host, segment, start.wrapping_add(count), &mut byte);
                if byte[0] == b'$' {
                    host.with_environment(|dos| dos.output.extend(output));
                    cpu.registers.eax = (cpu.registers.eax & !0xff) | 0x24;
                    host.set_cpu(cpu).unwrap();
                    return HostAction::Continue;
                }
                output.push(byte[0]);
            }
            return stop(
                host,
                DosStop::UnterminatedString {
                    segment,
                    offset: start,
                },
            );
        }
        0x25 => {
            let mut pointer = [0; 4];
            pointer[..2].copy_from_slice(&(cpu.registers.edx as u16).to_le_bytes());
            pointer[2..].copy_from_slice(&cpu.segments.ds.selector.to_le_bytes());
            host.write_memory(u32::from(cpu.registers.eax as u8) * 4, &pointer)
                .unwrap();
        }
        0x35 => {
            let mut pointer = [0; 4];
            host.read_memory(u32::from(cpu.registers.eax as u8) * 4, &mut pointer)
                .unwrap();
            cpu.registers.ebx = (cpu.registers.ebx & !0xffff)
                | u32::from(u16::from_le_bytes([pointer[0], pointer[1]]));
            cpu.segments.es =
                StoredSegment::real_mode(Segment::Es, u16::from_le_bytes([pointer[2], pointer[3]]));
        }
        0x40 => {
            let valid = matches!(cpu.registers.ebx as u16, 1 | 2);
            let result = if valid {
                let mut output = vec![0; usize::from(cpu.registers.ecx as u16)];
                read_segment(
                    host,
                    cpu.segments.ds.selector,
                    cpu.registers.edx as u16,
                    &mut output,
                );
                host.with_environment(|dos| dos.output.extend(output));
                cpu.registers.ecx as u16
            } else {
                6
            }; // invalid handle
            cpu.registers.eax = (cpu.registers.eax & !0xffff) | u32::from(result);
            return_carry(host, &cpu, !valid);
        }
        0x4c => return stop(host, DosStop::Exited(cpu.registers.eax as u8)),
        _ => return stop(host, DosStop::UnsupportedFunction(function)),
    }
    host.set_cpu(cpu)
        .expect("DOS changes preserve the real-mode context");
    HostAction::Continue
}

fn stop(host: &mut HostContext<'_, Dos>, reason: DosStop) -> HostAction {
    host.with_environment(|dos| dos.stop = Some(reason));
    HostAction::Yield
}

fn read_segment(host: &mut HostContext<'_, Dos>, segment: u16, offset: u16, bytes: &mut [u8]) {
    let first = bytes.len().min(0x10000 - usize::from(offset));
    let base = u32::from(segment) << 4;
    host.read_memory(base + u32::from(offset), &mut bytes[..first])
        .unwrap();
    if first < bytes.len() {
        host.read_memory(base, &mut bytes[first..]).unwrap();
    }
}

// IRET restores the caller's saved FLAGS; editing the handler's live CF would be lost.
fn return_carry(host: &mut HostContext<'_, Dos>, cpu: &CpuState, carry: bool) {
    let address = cpu.segments.ss.base + u32::from((cpu.registers.esp as u16).wrapping_add(4));
    let mut flags = [0; 2];
    host.read_memory(address, &mut flags).unwrap();
    flags[0] = (flags[0] & !1) | u8::from(carry);
    host.write_memory(address, &flags).unwrap();
}
