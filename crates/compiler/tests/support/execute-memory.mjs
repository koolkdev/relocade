import { readFileSync } from 'node:fs';

const [path, ...initializations] = process.argv.slice(2);
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
  result = instance.exports.run();
} catch (error) {
  if (!(error instanceof WebAssembly.RuntimeError)) throw error;
  result = 'trap';
}
const lines = memories.map(({ name, memory, length }) =>
  `${name}:${Buffer.from(memory.buffer, 0, length).toString('hex')}`
);
process.stdout.write(`${result}\n${lines.join('\n')}\n`);
