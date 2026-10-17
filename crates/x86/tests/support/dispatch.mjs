export default function execute([dispatcher, ...targets], [names, cases]) {
  const executionContext = new WebAssembly.Memory({ initial: 1 });
  const executionEntries = new WebAssembly.Memory({ initial: 1 });
  const executionFunctions = new WebAssembly.Table({ element: 'anyfunc', initial: targets.length + 1 });
  const imports = { wasm86: { executionContext, executionEntries, executionFunctions } };
  const instance = new WebAssembly.Instance(dispatcher, imports);
  Object.assign(imports.wasm86, instance.exports);
  targets.forEach((module, index) => {
    const target = new WebAssembly.Instance(module, imports);
    executionFunctions.set(index + 1, target.exports[names[index]]);
  });
  return cases.map(test => {
    const pages = Math.ceil(test.entries.length / 65536);
    const current = executionEntries.buffer.byteLength / 65536;
    if (pages > current) executionEntries.grow(pages - current);
    new Uint8Array(executionContext.buffer).set(test.cpu);
    new Uint8Array(executionEntries.buffer).set(test.entries);
    const word = instance.exports[test.export]();
    return { word: BigInt.asUintN(64, word).toString(), cpu: [...new Uint8Array(executionContext.buffer, 0, test.cpu.length)] };
  });
}
