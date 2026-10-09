import physicalMemory from './physical-memory.mjs';

export default function execute([module, interpreter], { entry, interpreter_entry, profile, invocations, input }) {
  const decode = ({ type, value }) => type === 'i64' ? BigInt(value) : value;
  const encode = value => typeof value === 'bigint'
    ? { type: 'i64', value: value.toString() } : { type: 'i32', value };
  const cpuState = new WebAssembly.Memory({ initial: 1 });
  const guest = new WebAssembly.Memory({ initial: 1 });
  const machine = new WebAssembly.Memory({ initial: 64 });
  new Uint8Array(cpuState.buffer).set(input.cpu);
  for (const [memory, patches] of [[guest, input.guest], [machine, input.machine]]) {
    for (const [offset, bytes] of patches) new Uint8Array(memory.buffer).set(bytes, offset);
  }
  const guestBefore = Buffer.from(new Uint8Array(guest.buffer));
  const moduleImports = WebAssembly.Module.imports(module);
  const observesMachine = interpreter !== undefined || moduleImports
    .some(resource => resource.module === 'wasm86' && resource.name === 'machine')
    || input.patches_before_calls.some(patches => patches.machine.length !== 0);
  const machineBefore = observesMachine ? Buffer.from(new Uint8Array(machine.buffer)) : null;
  let machineUnchanged = true;
  const snapshot = () => {
    machineUnchanged &&= machineBefore === null || machineBefore.equals(Buffer.from(machine.buffer));
    const changes = [];
    if (input.observe_guest) {
      const bytes = new Uint8Array(guest.buffer);
      for (let offset = 0; offset < bytes.length; offset++) {
        if (bytes[offset] !== guestBefore[offset]) changes.push([offset, bytes[offset]]);
      }
    }
    return {
      cpu: Array.from(new Uint8Array(cpuState.buffer, 0, input.cpu.length)),
      guest: input.observe_guest ? changes : null,
    };
  };
  const events = [];
  let resolutions = 0;
  let portReads = 0;
  let cpuidQueries = 0;
  let segmentQueries = 0;
  const physical = [module, interpreter].filter(Boolean)
    .some(module => WebAssembly.Module.imports(module).some(resource => resource.module === 'wasm86' && resource.name === 'physicalMap'))
    ? physicalMemory(guest, input, events) : null;
  const imports = {
    wasm86: {
      cpuState, guest, machine,
      ...physical?.imports,
      cpuid: (leaf, subleaf) => {
        if (cpuidQueries >= input.cpuid_results.length) throw new Error('unexpected CPUID query');
        events.push({kind: 'cpuid', leaf: leaf >>> 0, subleaf: subleaf >>> 0});
        return input.cpuid_results[cpuidQueries++];
      },
      readPort: (port, bytes) => {
        if (portReads >= input.port_reads.length) throw new Error('unexpected port read');
        events.push({kind: 'port_read', port, bytes});
        return input.port_reads[portReads++];
      },
      writePort: (port, bytes, value) => {
        events.push({kind: 'port_write', port, bytes, value: value >>> 0});
      },
      querySegmentDescriptor: selector => {
        const reply = input.segment_queries[segmentQueries++];
        if (!reply || reply.selector !== selector) {
          throw new Error(`unexpected segment query ${selector}`);
        }
        events.push({ kind: 'query_segment_descriptor', selector });
        return reply.values;
      },
      resolveSegment: (segment, selector) => {
        const reply = input.segment_resolutions[resolutions++];
        if (!reply || reply.segment !== segment || reply.selector !== selector) {
          throw new Error(`unexpected segment load ${segment}:${selector}`);
        }
        events.push({ kind: 'resolve_segment', segment, selector });
        return reply.values;
      },
      dispatch: eip => {
        events.push({ kind: 'dispatch', eip, snapshot: snapshot() });
        return BigInt(input.dispatch_return);
      },
      // Unlinked fixtures probe the boundary without executing the instruction.
      interpret: () => {
        events.push({ kind: 'interpret', snapshot: snapshot() });
        return BigInt(input.dispatch_return);
      },
    },
  };
  if (interpreter) {
    imports.wasm86.interpret = new WebAssembly.Instance(interpreter, imports).exports[interpreter_entry];
  }
  const instance = new WebAssembly.Instance(module, imports);
  const args = input.arguments.map(decode);
  for (let call = 0; call < invocations; call++) {
    const patches = input.patches_before_calls[call];
    if (patches) {
      for (const [memory, edits] of [[cpuState, patches.cpu], [guest, patches.guest], [machine, patches.machine]]) {
        for (const [offset, bytes] of edits) new Uint8Array(memory.buffer).set(bytes, offset);
      }
    }
    checkProfile();
    let outcome;
    try {
      const result = instance.exports[entry](...args);
      const values = result === undefined ? [] : Array.isArray(result) ? result : [result];
      outcome = { kind: 'returned', value: values.map(encode) };
    } catch (error) {
      if (!(error instanceof WebAssembly.RuntimeError)) throw error;
      outcome = { kind: 'trap' };
    }
    events.push({ kind: 'return', outcome, snapshot: snapshot() });
  }
  if (resolutions !== input.segment_resolutions.length) throw new Error('unused segment resolutions');
  if (segmentQueries !== input.segment_queries.length) {
    throw new Error('unused segment queries');
  }
  if (portReads !== input.port_reads.length) throw new Error('unused port reads');
  if (cpuidQueries !== input.cpuid_results.length) throw new Error('unused CPUID results');
  if (physical) physical.checkComplete();
  else if (input.mmio_updates.length !== 0) throw new Error('unused MMIO updates');
  return {
    events,
    guest_unchanged: guestBefore.equals(Buffer.from(guest.buffer)),
    machine_unchanged: machineUnchanged,
  };

  // Check the actual loaded caches after preceding calls and explicit host patches.
  function checkProfile() {
    if (profile === null) return;
    const cpu = new DataView(cpuState.buffer);
    const attributes = segment => cpu.getUint16(64 + segment * 12 + 10, true);
    const flat = (segment, kind) => {
      const offset = 64 + segment * 12;
      return cpu.getUint32(offset, true) === 0
        && cpu.getUint32(offset + 4, true) === 0xffffffff
        && (attributes(segment) & 15) === kind;
    };
    const codeBig = (attributes(1) & 16) !== 0;
    const real = segment => {
      const offset = 64 + segment * 12;
      return cpu.getUint32(offset, true) === cpu.getUint16(offset + 8, true) * 16
        && cpu.getUint32(offset + 4, true) === 0xffff
        && attributes(segment) === (segment === 1 ? 7 : 5);
    };
    const compatible = codeBig === !['segmented16', 'real16'].includes(profile)
      && (profile !== 'real16' || [0, 1, 2, 3, 4, 5].every(real))
      && (profile !== 'flat32' || (flat(1, 7) && [0, 2, 3].every(s => flat(s, 5))
        && (attributes(2) & 16) !== 0));
    if (!compatible) throw new Error(`${entry} requires compatible ${profile} segment state`);
  }
}
