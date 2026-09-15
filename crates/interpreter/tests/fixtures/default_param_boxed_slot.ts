// A synthesized default for a parameter whose slot is a reference type
// (`unknown`, a union, `T | null`) has to be boxed like any other primitive
// argument — the literal's own type drives the call-boundary coercion.
enum Color {
  Red = 0,
  Blue = 1,
}

function un(x: unknown = 5, s: unknown = "hi", b: unknown = true): string {
  if (typeof x === "number" && typeof s === "string" && typeof b === "boolean") {
    return `${x}${s}${b}`;
  }
  return "wrong";
}

function widened(n: number | string = 7, o: number | null = -3, c: Color | null = Color.Blue): number {
  const left = typeof n === "number" ? n : 0;
  const right = o === null ? 0 : o;
  const tone = c === null ? -1 : (c === Color.Blue ? 1 : 0);
  return left + right + tone;
}

class Holder {
  x: unknown;
  y: number | string;
  constructor(x: unknown = 5, y: number | string = 7) {
    this.x = x;
    this.y = y;
  }
  bump(by: unknown = 2): boolean {
    return typeof by === "number";
  }
}

class Prop {
  constructor(readonly v: unknown = 9) {}
  static make(seed: unknown = 4): Prop {
    return new Prop(seed);
  }
}

function generic<T>(item: T, extra: unknown = 6): boolean {
  return typeof extra === "number";
}

class GenericHolder<T> {
  item: T;
  extra: unknown;
  constructor(item: T, extra: unknown = 8) {
    this.item = item;
    this.extra = extra;
  }
  widen(x: unknown = 3): boolean {
    return typeof x === "number";
  }
}

class Sub extends Holder {
  constructor() {
    super();
  }
}

function main(): void {
  assert(un() === "5hitrue", "number/string/boolean defaults into `unknown` slots");
  assert(un(1, "a", false) === "1afalse", "explicit args still fill those slots");
  assert(widened() === 5, "defaults into union, nullable, and enum-or-null slots");

  const h = new Holder();
  assert(typeof h.x === "number" && h.x === 5, "ctor default into an `unknown` slot");
  assert(typeof h.y === "number" && h.y === 7, "ctor default into a union slot");
  assert(h.bump(), "method default into an `unknown` slot");
  assert(new Prop().v === 9, "parameter-property default into an `unknown` slot");
  assert(Prop.make().v === 4, "static-method default into an `unknown` slot");
  assert(new Sub().x === 5, "`super()` default into an `unknown` slot");

  assert(generic<string>("a"), "generic function default into an `unknown` slot");
  const gh = new GenericHolder<string>("a");
  assert(gh.extra === 8, "generic class ctor default into an `unknown` slot");
  assert(gh.widen(), "generic class method default into an `unknown` slot");
}
