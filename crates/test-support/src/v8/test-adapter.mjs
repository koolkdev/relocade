const seen = new WeakSet();

export default function execute(module, input) {
  if (input.exit) process.exit(1);
  if (input.error) throw new Error(input.error);
  const reused = seen.has(module);
  seen.add(module);
  const instance = new WebAssembly.Instance(module);
  return { result: instance.exports.run(input.delta), reused };
}
