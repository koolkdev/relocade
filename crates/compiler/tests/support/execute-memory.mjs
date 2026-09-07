import { readFileSync } from 'node:fs';

const [path, ...arguments_] = process.argv.slice(2);
const separator = arguments_.indexOf('--');
const initializations = separator < 0 ? arguments_ : arguments_.slice(0, separator);
const inputs = separator < 0 ? [] : arguments_.slice(separator + 1).map(text => {
  const [type, value] = text.split(':');
  return type === 'i64' ? BigInt(value) : Number(value);
});
const imports = { test: {} };
const memories = initializations.map(initialization => {
  const [name, hex] = initialization.split(':');
  const initial = Buffer.from(hex, 'hex');
  const memory = new WebAssembly.Memory({ initial: 1 });
  new Uint8Array(memory.buffer).set(initial);
  imports.test[name] = memory;
  return { name, memory, length: initial.length };
});
const module = new WebAssembly.Module(readFileSync(path));
const instance = new WebAssembly.Instance(module, imports);
let result;
try {
  result = instance.exports.run(...inputs);
} catch (error) {
  if (!(error instanceof WebAssembly.RuntimeError)) throw error;
  result = 'trap';
}
const lines = memories.map(({ name, memory, length }) =>
  `${name}:${Buffer.from(memory.buffer, 0, length).toString('hex')}`
);
process.stdout.write(`${result}\n${lines.join('\n')}\n`);
