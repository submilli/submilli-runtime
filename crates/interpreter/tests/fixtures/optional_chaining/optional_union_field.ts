// A `?.` field read resolves against a union of field-bearing members exactly as
// the plain `.` read does: the name must exist on every member, and the step's
// type is the union of the per-member field types. The plain read of the same
// union is the control every case here is checked against.

type A = { kind: "a"; v: number };
type B = { kind: "b"; v: number; extra: number };

interface HasV {
  v: number;
}
class Klass {
  v: number = 7;
}

type Wrap = { v: { w: number } };
type Wrap2 = { v: { w: number }; tag: number };

interface I1 {
  v: number;
}
interface I2 {
  v: number;
  other: number;
}
class K1 {
  v: number = 11;
}
class K2 {
  pad: string = "p";
  other: number = 1;
  v: number = 12;
}

// One member declares the field optional, so the step widens to `| undefined`.
type Opt = { v?: number };
type Req = { v: number };

// The field sits at a different payload slot in each member; the read is a
// runtime name scan, so no member's layout is privileged.
type Front = { v: number[]; z: number };
type Back = { a: number; b: number; v: number[] };

type Callable = { v: () => number };
type Callable2 = { v: () => number; tag: number };

type Nested = { inner: A | B };

function readChain(u: A | B | null): number | undefined {
  return u?.v;
}
function readPlain(u: A | B): number {
  return u.v;
}

function mkA(): A {
  return { kind: "a", v: 1 };
}
function mkB(): B {
  return { kind: "b", v: 2, extra: 3 };
}
function mkWrap(): Wrap {
  return { v: { w: 5 } };
}
function mkOpt(): Opt {
  return {};
}
function mkFront(): Front {
  return { v: [1, 2, 3], z: 0 };
}
function mkBack(): Back {
  return { a: 0, b: 0, v: [4, 5, 6] };
}
function mkCallable(): Callable {
  return { v: (): number => 42 };
}
function mkNested(): Nested {
  return { inner: mkA() };
}
function mkUnion(): A | B | null {
  return mkB();
}

function main(): void {
  assert(readPlain(mkA()) === 1, "the plain union read is the control");
  assert(readChain(mkA()) === 1, "a `?.` read off a union of object shapes");
  assert(readChain(mkB()) === 2, "the other member of the same union");
  assert(readChain(null) === undefined, "a null receiver short-circuits the union step");

  // A union with a nominal member lowers to the universal `$Object` slot, so the
  // step has to downcast to `$ObjectShape` before the field-name scan — the same
  // coercion the plain path makes.
  const objOrClass: A | Klass | null = new Klass();
  assert(objOrClass?.v === 7, "object shape | class");

  const iface: HasV = { v: 3 };
  const objOrIface: A | HasV | null = iface;
  assert(objOrIface?.v === 3, "object shape | interface");

  const classOrIface: Klass | HasV | null = iface;
  assert(classOrIface?.v === 3, "class | interface");

  const ifaces: I1 | I2 | null = iface;
  assert(ifaces?.v === 3, "a union of two interfaces");
  const classes: K1 | K2 | null = new K2();
  assert(classes?.v === 12, "a union of two classes");

  // Optional on one side, required on the other: the step type carries undefined.
  const opt: Opt | Req | null = mkOpt();
  assert(opt?.v === undefined, "an optional field on one member widens the step");

  // Different payload slots per member.
  const front: Front | Back | null = mkFront();
  const back: Front | Back | null = mkBack();
  assert(front?.v[0] === 1, "the field at a low slot");
  assert(back?.v[0] === 4, "the same field at a high slot");
  assert(front?.v.length === 3, "a `.length` read after a union step");

  // Every step kind after the union step.
  const w: Wrap | Wrap2 | null = mkWrap();
  assert(w?.v.w === 5, "a plain step after a union step");
  assert(w?.v?.w === 5, "an optional step after a union step");
  assert(w?.v.w! === 5, "a `!` after a union step");
  const c: Callable | Callable2 | null = mkCallable();
  assert(c?.v() === 42, "a call after a union step");
  const gone: Callable | Callable2 | null = null as Callable | Callable2 | null;
  assert((gone?.v() ?? 7) === 7, "a `??` tail over a short-circuited union step");

  // The union step in the middle of a chain rather than at its head.
  const n: Nested | null = mkNested();
  assert(n?.inner.v === 1, "a union step at step 2");

  // A union receiver that is not a local: an array element, and a call result.
  const arr: (A | B | null)[] = [mkB()];
  assert(arr[0]?.v === 2, "a union receiver from an array element");
  assert(mkUnion()?.v === 2, "a union receiver from a call");
}
