import { readFileSync } from 'node:fs';

const { entry, invocations, input } = JSON.parse(readFileSync(0, 'utf8'));
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
const machineBefore = Buffer.from(new Uint8Array(machine.buffer));
const snapshot = () => {
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
const module = new WebAssembly.Module(readFileSync(process.argv[2]));
const instance = new WebAssembly.Instance(module, {
  wasm86: {
    cpuState, guest, machine,
    dispatch: eip => {
      events.push({ kind: 'dispatch', eip, snapshot: snapshot() });
      return BigInt(input.dispatch_return);
    },
  },
});
const args = input.arguments.map(decode);
for (let call = 0; call < invocations; call++) {
  for (const [offset, bytes] of input.cpu_patches_before_calls[call] ?? []) {
    new Uint8Array(cpuState.buffer).set(bytes, offset);
  }
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
process.stdout.write(JSON.stringify({
  events,
  guest_unchanged: guestBefore.equals(Buffer.from(guest.buffer)),
  machine_unchanged: machineBefore.equals(Buffer.from(machine.buffer)),
}));
