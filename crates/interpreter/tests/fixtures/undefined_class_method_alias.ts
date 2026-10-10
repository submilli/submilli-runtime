interface Reader { read?(): number; }
class Box implements Reader { read(): number { return 8; } }
class Derived extends Box { extra: number = 1; }
function replace(reader: Reader): void { reader.read = (): number => 9; }
function clearRequired(reader: Reader): void { reader.read = undefined; }

interface OptionalReader { base: number; read?(value: number): number; }
class OptionalBox implements OptionalReader {
  base: number = 10;
  read?(value: number): number { return this.base + value; }
}
function replaceWithReceiver(reader: OptionalReader): void {
  reader.read = function(this: OptionalReader, value: number): number {
    return this.base + value + 1;
  };
}
function clear(reader: OptionalReader): void { reader.read = undefined; }
let effects = 0;
function argument(): number { effects += 1; return 8; }
function readOptional(receiver: OptionalBox | null): number | undefined {
  return receiver?.read?.(argument());
}

interface Printable { label: string; toString?(): string; }
class PrintableBox implements Printable {
  label: string = "new";
  toString(): string { return "old"; }
}
class DerivedPrintable extends PrintableBox { extra: number = 1; }
function replaceStringConversion(value: Printable): void {
  value.toString = function(this: Printable): string { return this.label; };
}
function clearStringConversion(value: Printable): void { value.toString = undefined; }
function checkStringConversion(value: PrintableBox): void {
  replaceStringConversion(value);
  assert(value.toString() === "new", "direct conversion reads the replacement");
  assert(String(value) === "new", "String reads the replacement and binds its receiver");
  const erased: unknown = value;
  assert(String(erased) === "new", "erased conversion reads the replacement");
  assert(`${value}` === "new", "interpolation reads the replacement");
  clearStringConversion(value);
  let caught = false;
  try {
    String(value);
  } catch (error) {
    assert(error instanceof TypeError);
    caught = true;
  }
  assert(caught, "clearing the conversion raises a catchable TypeError");
}

function main(): void {
  const box = new Box();
  const alias: Reader = box;
  replace(alias);
  assert(alias.read?.() === 9 && box.read() === 9, "class and interface read the same replacement");
  clearRequired(alias);
  assert(box.read?.() === undefined, "optional call observes alias clearing a declared method");
  const derived = new Derived();
  replace(derived);
  const parent: Box = derived;
  assert(parent.read() === 9 && derived.read() === 9, "inherited method payload follows dynamic layout");
  const optional = new OptionalBox();
  replaceWithReceiver(optional);
  assert(optional.read?.(2) === 13, "replacement call keeps its current receiver");
  clear(optional);
  assert(optional.read?.(argument()) === undefined, "cleared method reads undefined");
  assert(readOptional(optional) === undefined && readOptional(null) === undefined);
  assert(effects === 0, "optional calls skip arguments when receiver or method is absent");
  checkStringConversion(new PrintableBox());
  checkStringConversion(new DerivedPrintable());
}
