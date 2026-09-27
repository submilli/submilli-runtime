class Reader {
  value: number = 10;
  read(x: number = 4): number { return this.value + x; }
  mark(x: number = 2): void { this.value = x; }
}
class PlainReader { read(x: number = 14): number { return x; } }
interface Nullable { read: (() => number) | null; }
function inline(value: {read(): number; mark(): void}): void {
  assert(value.read() === 14, "inline default");
  value.mark();
  assert(value.read() === 6, "void default");
}
function invoke(f: () => number): number { return f(); }
function nullable(value: Nullable): void {
  const fn = value.read;
  if (fn !== null) {
    assert(fn() === 14, "extracted default");
    assert(invoke(fn) === 14, "forwarded default");
  }
  assert(value.read?.() === 14, "optional call default");
}
class Mapper { read(x: number, unused: number = 10): number { return x + 10; } }
function map(reader: { read: ((x: number) => number) | null }): number[] {
  const fn = reader.read;
  if (fn === null) return [];
  return [1, 2].map(fn);
}
function main(): void {
  assert(JSON.stringify(map(new Mapper())) === '[11,12]', "higher-order default callback");
  inline(new Reader());
  nullable(new PlainReader());
}
