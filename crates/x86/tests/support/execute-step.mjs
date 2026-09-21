export default function execute(module, { entry, profile, invocations, input }) {
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
  const observesMachine = WebAssembly.Module.imports(module)
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
  let segmentQueries = 0;
  const instance = new WebAssembly.Instance(module, {
    wasm86: {
      cpuState, guest, machine,
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
    },
  });
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
  return {
    events,
    guest_unchanged: guestBefore.equals(Buffer.from(guest.buffer)),
    machine_unchanged: machineUnchanged,
  };

  // Check the actual loaded caches after preceding calls and explicit host patches.
  function checkProfile() {
    if (profile === null) return;
    const cpu = new DataView(cpuState.buffer);
    const attributes = segment => cpu.getUint16(60 + segment * 12 + 10, true);
    const flat = (segment, kind) => {
      const offset = 60 + segment * 12;
      return cpu.getUint32(offset, true) === 0
        && cpu.getUint32(offset + 4, true) === 0xffffffff
        && (attributes(segment) & 15) === kind;
    };
    const codeBig = (attributes(1) & 16) !== 0;
    const compatible = codeBig === (profile !== 'segmented16')
      && (profile !== 'flat32' || (flat(1, 7) && [0, 2, 3].every(s => flat(s, 5))
        && (attributes(2) & 16) !== 0));
    if (!compatible) throw new Error(`${entry} requires compatible ${profile} segment state`);
  }
}
