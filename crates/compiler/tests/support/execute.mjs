export default function execute(module, input) {
  const value = scalar => scalar.type === 'i64' ? BigInt(scalar.value) : scalar.value;
  const scalar = value => typeof value === 'bigint'
    ? { type: 'i64', value: value.toString() }
    : { type: 'i32', value };
  const imports = { test: {} };
  const memoryImports = new Map(input.memory_imports.map(({ name, ...descriptor }) => [name, descriptor]));
  const memories = input.memories.map(({ name, bytes }) => {
    const memory = new WebAssembly.Memory(memoryImports.get(name) ?? { initial: 1 });
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
      const results = callback.results.map(value);
      return results.length === 0 ? undefined : results.length === 1 ? results[0] : results;
    };
  }
  const instance = new WebAssembly.Instance(module, imports);
  let outcome;
  try {
    const result = instance.exports[input.entry](...input.arguments.map(value));
    const results = result === undefined ? [] : Array.isArray(result) ? result : [result];
    outcome = { kind: 'returned', value: results.map(scalar) };
  } catch (error) {
    if (!(error instanceof WebAssembly.RuntimeError)) throw error;
    outcome = { kind: 'trap' };
  }
  return { outcome, callbacks, memories: snapshot() };
}
