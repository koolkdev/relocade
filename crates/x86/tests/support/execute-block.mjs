import { readFileSync } from 'node:fs';

const [path, entry, initial, returned] = process.argv.slice(2);
const bytes = Buffer.from(initial, 'hex');
const memory = new WebAssembly.Memory({ initial: 1 });
new Uint8Array(memory.buffer).set(bytes);
const snapshot = () => Buffer.from(memory.buffer, 0, bytes.length).toString('hex');
const lines = [];
const module = new WebAssembly.Module(readFileSync(path));
const instance = new WebAssembly.Instance(module, {
  wasm86: {
    cpuState: memory,
    dispatch: (...args) => {
      lines.push(`dispatch(${args.join(',')}) ${snapshot()}`);
      return BigInt(returned);
    },
  },
});
const result = instance.exports[entry]();
lines.push(`return ${result}`, `state ${snapshot()}`);
process.stdout.write(`${lines.join('\n')}\n`);
