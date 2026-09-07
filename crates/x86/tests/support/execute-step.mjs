import { readFileSync } from 'node:fs';

const [path, entry, returned, invocations = '1'] = process.argv.slice(2);
const [cpu, guestPatches, machinePatches, arguments_ = [], observeGuest = false] = JSON.parse(readFileSync(0, 'utf8'));
const args = arguments_.map(([type, value]) => type === 'i64' ? BigInt(value) : Number(value));
const cpuState = new WebAssembly.Memory({ initial: 1 });
const guest = new WebAssembly.Memory({ initial: 1 });
const machine = new WebAssembly.Memory({ initial: 64 });
const snapshot = () => Buffer.from(cpuState.buffer, 0, cpu.length).toString('hex');
const lines = [];
const module = new WebAssembly.Module(readFileSync(path));
const instance = new WebAssembly.Instance(module, {
  wasm86: {
    cpuState, guest, machine,
    dispatch: (...args) => {
      lines.push(`dispatch(${args.join(',')}) ${snapshot()}`);
      if (observeGuest) lines.push(`guest at dispatch ${guestChanges()}`);
      return BigInt(returned);
    },
  },
});
new Uint8Array(cpuState.buffer).set(cpu);
for (const [memory, patches] of [[guest, guestPatches], [machine, machinePatches]]) {
  for (const [offset, bytes] of patches) new Uint8Array(memory.buffer).set(bytes, offset);
}
const guestBefore = Buffer.from(new Uint8Array(guest.buffer));
const guestChanges = () => {
  const bytes = new Uint8Array(guest.buffer);
  const changes = [];
  for (let offset = 0; offset < bytes.length; offset++) {
    if (bytes[offset] !== guestBefore[offset]) changes.push([offset, bytes[offset]]);
  }
  return JSON.stringify(changes);
};
const machineBefore = Buffer.from(new Uint8Array(machine.buffer));
for (let call = 0; call < Number(invocations); call++) {
  let result;
  try {
    result = instance.exports[entry](...args);
  } catch (error) {
    if (!(error instanceof WebAssembly.RuntimeError)) throw error;
    result = 'trap';
  }
  lines.push(`return ${result}`, `state ${snapshot()}`);
  if (observeGuest) lines.push(`guest at return ${guestChanges()}`);
}
lines.push(`guest ${guestBefore.equals(Buffer.from(guest.buffer)) ? 'unchanged' : 'changed'}`);
lines.push(`machine ${machineBefore.equals(Buffer.from(machine.buffer)) ? 'unchanged' : 'changed'}`);
process.stdout.write(`${lines.join('\n')}\n`);
