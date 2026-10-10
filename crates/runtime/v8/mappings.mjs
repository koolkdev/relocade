// Canonical mapping metadata and reverse backing aliases for one memory owner.
const UNMAPPED = Object.freeze({ kind: 'unmapped' });
const WATCH = 0x10;

export class Mappings {
  #table;
  #physical;
  #pages = new Map();
  #aliases = new Map();

  constructor(table, physical) {
    this.#table = table;
    this.#physical = physical;
    const view = new DataView(table.buffer);
    for (let page = 0; page < this.pageCount; page++) {
      const offset = this.#offset(page);
      const word = view.getUint32(offset, true);
      if (word & WATCH) throw new Error('a mapping set has one code owner');
      let mapping = UNMAPPED;
      if (physical) {
        if (word === 1 || word === 2) mapping = { kind: 'ram', backing: view.getUint32(offset + 4, true), writable: word === 1 };
        else if (word === 3) mapping = { kind: 'mmio' };
        else if (word !== 0) throw new Error('invalid physical mapping');
      } else if (word & 1) {
        mapping = { kind: 'ram', backing: (word & ~4095) >>> 0, writable: !!(word & 2) };
      }
      if (mapping !== UNMAPPED) this.replace(page, mapping);
    }
  }

  get pageCount() { return this.#physical ? 272 : 1 << 20; }
  #offset(page) { return page * (this.#physical ? 8 : 4); }
  get(page) { return this.#pages.get(page) ?? UNMAPPED; }

  validate(page, mapping) {
    if (!Number.isInteger(page) || page < 0 || page >= this.pageCount) throw new RangeError('invalid mapping page');
    if (!['unmapped', 'ram', 'mmio'].includes(mapping.kind)) throw new TypeError('invalid mapping kind');
    if (mapping.kind === 'mmio' && !this.#physical) throw new TypeError('virtual mappings cannot route devices');
    if (mapping.kind === 'ram' && (!Number.isInteger(mapping.backing) || mapping.backing < 0
      || mapping.backing > 0xfffff000 || (mapping.backing & 4095) !== 0 || typeof mapping.writable !== 'boolean')) {
      throw new TypeError('RAM mapping needs aligned backing and writability');
    }
  }

  replace(page, mapping) {
    this.validate(page, mapping);
    const previous = this.get(page);
    if (previous.kind === 'ram') {
      const aliases = this.#aliases.get(previous.backing);
      aliases.delete(page);
      if (aliases.size === 0) this.#aliases.delete(previous.backing);
    }
    if (mapping.kind === 'ram') {
      if (!this.#aliases.has(mapping.backing)) this.#aliases.set(mapping.backing, new Set());
      this.#aliases.get(mapping.backing).add(page);
    }
    if (mapping.kind === 'unmapped') this.#pages.delete(page);
    else this.#pages.set(page, { ...mapping });
  }

  write(page, watched) {
    const mapping = this.get(page);
    let word = 0;
    if (mapping.kind === 'ram') word = this.#physical ? (mapping.writable ? 1 : 2)
      : mapping.backing | 1 | (mapping.writable ? 2 : 0);
    else if (mapping.kind === 'mmio') word = 3;
    const view = new DataView(this.#table.buffer);
    view.setUint32(this.#offset(page), word | (watched ? WATCH : 0), true);
    if (this.#physical) view.setUint32(this.#offset(page) + 4, mapping.backing ?? 0, true);
  }

  watch(backing, watched) {
    for (const page of this.#aliases.get(backing) ?? []) this.write(page, watched);
  }
}
