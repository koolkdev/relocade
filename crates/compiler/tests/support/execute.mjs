import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const [path, name, ...arguments_] = process.argv.slice(2);
const module = new WebAssembly.Module(readFileSync(path));
const instance = new WebAssembly.Instance(module);
const args = arguments_.map(argument => {
  const [type, value] = argument.split(':');
  if (type === 'i64') return BigInt(value);
  assert.equal(type, 'i32');
  return Number(value);
});
process.stdout.write(`${instance.exports[name](...args)}\n`);
