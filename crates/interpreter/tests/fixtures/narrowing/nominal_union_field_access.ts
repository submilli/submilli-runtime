// Reading a field off a union whose members are *named* — interfaces, classes,
// or a mix. The members lay the field out at different slots, so the read goes
// through the same runtime field-name scan an object-type union uses; class
// instances qualify because their struct header is the `$ObjectShape` prefix.
//
// Discriminating such a union — by a literal tag or by `in` — depends on the
// same expansion, so both are covered here.

interface IA {
  x: number;
}

interface IB {
  x: number;
  y: number;
}

class CA {
  constructor(
    readonly x: number,
    readonly tag: string,
  ) {}
}

class CB {
  // Deliberately a different declaration order, so a static slot index would
  // read the wrong field.
  constructor(
    readonly tag: string,
    readonly x: number,
  ) {}
}

class WithAccessor {
  private v: number = 5;
  get x(): number {
    return this.v * 2;
  }
}

class Plain {
  x: number = 1;
}

interface Ok {
  kind: "ok";
  value: number;
}

interface Err {
  kind: "err";
  message: string;
}

type Result = Ok | Err;

interface HasA {
  a: number;
}

interface HasB {
  b: number;
}

interface Maybe {
  x?: number;
}

function readInterfaces(v: IA | IB): number {
  return v.x;
}

function readClasses(v: CA | CB): number {
  return v.x;
}

function readMixed(v: IA | Plain): number {
  return v.x;
}

function readAccessor(v: WithAccessor | Plain): number {
  return v.x;
}

function byTag(r: Result): string {
  if (r.kind === "ok") {
    return `${r.value}`;
  }
  return r.message;
}

function bySwitch(r: Result): string {
  switch (r.kind) {
    case "ok":
      return `ok:${r.value}`;
    default:
      return `err:${r.message}`;
  }
}

function byPresence(v: HasA | HasB): number {
  if ("a" in v) {
    return v.a;
  }
  return v.b;
}

// One member's field is optional, so the read widens to `number | undefined`.
function optionalMember(v: Maybe | IA): string {
  const r = v.x;
  return r === undefined ? "absent" : `${r}`;
}

// Class members with literal-typed tags: the discriminant analysis expands a
// `ClassRef` through the type registry, same as an interface.
class OkC {
  readonly kind: "ok" = "ok";
  constructor(readonly value: number) {}
}

class ErrC {
  readonly kind: "err" = "err";
  constructor(readonly message: string) {}
}

function byClassTag(r: OkC | ErrC): string {
  if (r.kind === "ok") {
    return `${r.value}`;
  }
  return r.message;
}

// A subclass and its base in one union: the field sits at a different offset in
// each, and the read still resolves by name.
class Base {
  x: number = 1;
}

class Derived extends Base {
  extra: string = "e";
}

function baseOrDerived(v: Base | Derived): number {
  return v.x;
}

// Carrier positions: array element, `Map` value, and a returned union.
function fromArray(): number {
  const arr: Array<IA | CA> = [{ x: 1 }, new CA(2, "t")];
  let sum = 0;
  for (const v of arr) {
    sum = sum + v.x;
  }
  return sum;
}

function fromMap(): number {
  const m = new Map<string, IA | CA>();
  m.set("a", new CA(5, "t"));
  return m.get("a")!.x;
}

function returned(flag: boolean): IA | CA {
  return flag ? { x: 1 } : new CA(2, "t");
}

function main(): void {
  assert(readInterfaces({ x: 1 }) === 1, "union of interfaces");
  assert(readInterfaces({ x: 2, y: 3 }) === 2, "second interface member");
  assert(readClasses(new CA(7, "a")) === 7, "class member, field first");
  assert(readClasses(new CB("b", 9)) === 9, "class member, field second");
  assert(readMixed(new Plain()) === 1, "class in an interface union");
  assert(readMixed({ x: 4 }) === 4, "object literal in the same union");
  assert(readAccessor(new WithAccessor()) === 10, "accessor-backed member");
  assert(readAccessor(new Plain()) === 1, "data-field member beside it");

  assert(byTag({ kind: "ok", value: 5 }) === "5", "literal discriminant, ok");
  assert(byTag({ kind: "err", message: "bad" }) === "bad", "literal discriminant, err");
  assert(bySwitch({ kind: "ok", value: 6 }) === "ok:6", "switch discriminant, ok");
  assert(bySwitch({ kind: "err", message: "no" }) === "err:no", "switch discriminant, err");

  assert(byPresence({ a: 11 }) === 11, "`in` narrowing on named members");
  assert(byPresence({ b: 12 }) === 12, "`in` narrowing, other side");

  assert(optionalMember({ x: 8 }) === "8", "optional member present");
  assert(optionalMember({}) === "absent", "optional member absent");

  assert(byClassTag(new OkC(4)) === "4", "class-literal discriminant, ok");
  assert(byClassTag(new ErrC("no")) === "no", "class-literal discriminant, err");
  assert(baseOrDerived(new Base()) === 1, "base member");
  assert(baseOrDerived(new Derived()) === 1, "subclass member, inherited field");
  assert(fromArray() === 3, "union as an array element");
  assert(fromMap() === 5, "union as a Map value");
  assert(returned(true).x === 1 && returned(false).x === 2, "union as a return type");
}
