import { readFileSync } from 'node:fs';

const input = JSON.parse(readFileSync(0, 'utf8'));
const value = scalar => scalar.type === 'i64' ? BigInt(scalar.value) : scalar.value;
const scalar = value => typeof value === 'bigint'
  ? { type: 'i64', value: value.toString() }
  : { type: 'i32', value };
const imports = { test: {} };
const memories = input.memories.map(({ name, bytes }) => {
  const memory = new WebAssembly.Memory({ initial: 1 });
  new Uint8Array(memory.buffer).set(bytes);
  imports.test[name] = memory;
  return { name, memory, length: bytes.length };
});
const snapshot = () => memories.map(({ name, memory, length }) => ({
  name,
  bytes: Array.from(new Uint8Array(memory.buffer, 0, length)),
}));
const callbacks = [];
for (const callback of input.callbacks) {
  imports.test[callback.name] = (...arguments_) => {
    callbacks.push({ name: callback.name, arguments: arguments_.map(scalar), memories: snapshot() });
    return callback.result === null ? undefined : value(callback.result);
  };
}
const module = new WebAssembly.Module(readFileSync(process.argv[2]));
const instance = new WebAssembly.Instance(module, imports);
let outcome;
try {
  const result = instance.exports[input.entry](...input.arguments.map(value));
  outcome = { kind: 'returned', value: result === undefined ? null : scalar(result) };
} catch (error) {
  if (!(error instanceof WebAssembly.RuntimeError)) throw error;
  outcome = { kind: 'trap' };
}
process.stdout.write(JSON.stringify({ outcome, callbacks, memories: snapshot() }));
