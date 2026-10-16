export default function execute([dispatcher, targets], [dataOffset, observeOffset, cases]) {
  const executionContext = new WebAssembly.Memory({ initial: 1 });
  const executionEntries = new WebAssembly.Memory({ initial: 1 });
  const executionFunctions = new WebAssembly.Table({ element: 'anyfunc', initial: 4 });
  const instance = new WebAssembly.Instance(dispatcher, {
    wasm86: { executionContext, executionEntries, executionFunctions },
  });
  const callees = new WebAssembly.Instance(targets);
  [instance.exports.enter, callees.exports.fallback, callees.exports.first, callees.exports.second]
    .forEach((entry, index) => executionFunctions.set(index, entry));
  return cases.map(test => {
    const pages = Math.ceil((dataOffset + test.entries.length) / 65536);
    const current = executionEntries.buffer.byteLength / 65536;
    if (pages > current) executionEntries.grow(pages - current);
    new Uint8Array(executionContext.buffer).set(test.cpu);
    new Uint8Array(executionEntries.buffer).set(test.entries, dataOffset);
    new DataView(executionEntries.buffer).setUint32(observeOffset, Number(test.observe), true);
    return instance.exports[test.export](...(test.export === 'dispatch' ? [0] : [])).toString();
  });
}
