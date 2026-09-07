import { readFileSync } from 'node:fs';

const [path, exported, initial, callbacks, ...inputs] = process.argv.slice(2);
const scalar = text => {
  const [type, value] = text.split(':');
  return type === 'i64' ? BigInt(value) : Number(value);
};
const imports = { test: {} };
const lines = [];
let snapshot = () => '';
if (initial !== '-') {
  const bytes = Buffer.from(initial, 'hex');
  const memory = new WebAssembly.Memory({ initial: 1 });
  new Uint8Array(memory.buffer).set(bytes);
  imports.test.state = memory;
  snapshot = () => Buffer.from(memory.buffer, 0, bytes.length).toString('hex');
}
for (const callback of callbacks.split(',').filter(Boolean)) {
  const [name, ...result] = callback.split(':');
  imports.test[name] = (...args) => {
    lines.push(`${name}(${args.join(',')}) ${snapshot()}`.trimEnd());
    return scalar(result.join(':'));
  };
}
const module = new WebAssembly.Module(readFileSync(path));
const instance = new WebAssembly.Instance(module, imports);
let result;
try {
  result = instance.exports[exported](...inputs.map(scalar));
} catch (error) {
  if (!(error instanceof WebAssembly.RuntimeError)) throw error;
  result = 'trap';
}
lines.push(`return ${result}`);
if (initial !== '-') lines.push(`state ${snapshot()}`);
process.stdout.write(`${lines.join('\n')}\n`);
