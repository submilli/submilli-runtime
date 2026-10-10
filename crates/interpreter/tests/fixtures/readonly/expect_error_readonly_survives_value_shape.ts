// A write narrows a binding with `readonly` in its declaration to the declared
// member, so the declaration's `readonly` survives wherever the value's type
// puts its own: in a different field, inside a type argument, in a function
// type's return, inside a recursive or mutually recursive alias, or in a
// function-valued property of an object type, an interface (with or without
// methods), or a class. A value's own `readonly` survives too: a subclass that
// makes an inherited field `readonly` keeps it. Of two members differing only in
// a field's `readonly`, neither stands for the value alone, and of two differing
// in their fields, the one listing fewer does, at any depth: inside a field's
// object type, inside a nullable element, and alternative by alternative
// inside a union. Each of the seventeen writes below matches a `tsc --strict`
// error.
// expect-error-count: 17
// expect-error: cannot call `push` on `readonly number[]`
// expect-error: cannot assign to readonly property `xs`
// expect-error: cannot assign to readonly property `val`
// expect-error: cannot assign to readonly field `x` on `Sub`
// expect-error: cannot assign to readonly property `cb`
// expect-error: cannot assign to readonly field `f` on `Gadget`
// expect-error: no field `extra` on type `IdOnly`
// expect-error: no field `y`
// expect-error: field `s` does not exist on all members

interface Record1 {
  readonly id: number;
  xs: number[];
}
interface Declared1 {
  readonly id: number;
  xs: readonly number[];
}
type Getter = { get: () => readonly number[] };
type Chain<X> = { v: X; next: Chain<readonly X[]> | null };
type Tree = { readonly val: number; kids: Tree[] };
type Outer = { inner: Inner };
type Inner = { xs: readonly number[]; next: Outer | null };

class Base {
  x: number = 1;
}
class Sub extends Base {
  readonly x: number = 2;
}

interface Callback {
  readonly cb: () => number;
}
interface Handler {
  readonly f: () => number;
  run(): number;
}
class Runner implements Handler {
  f: () => number = (): number => 1;
  run(): number {
    return 2;
  }
}
type Wrapped = { readonly f: (n: number) => number; tag: number };

class Gadget {
  readonly f: () => number = (): number => 1;
  n: number = 0;
}
class Widget extends Gadget {
  f: () => number = (): number => 2;
  readonly n: number = 3;
}
type WithExtra = { readonly id: number; extra?: number };
type IdOnly = { readonly id: number };

interface Roomy {
  readonly id: number;
  p: { x: number; y?: number };
}
interface Tight {
  readonly id: number;
  p: { x: number };
}
type Wide = { x: number; y?: number };
type Narrow = { x: number };

type C1 = { k: number; a?: number; s?: number };
type C2 = { k: number; b?: number; s?: number };
type G1 = { k: number; a?: number; s?: number; x1?: number };
type G2 = { k: number; b?: number; s?: number; x2?: number };
type Bare = { k: number; a0?: number };
type Covered = { readonly id: number; x: C1 | C2 };
type Uncovered = { readonly id: number; x: G1 | Bare | G2 };

interface Mutable {
  a: number;
}
interface Frozen {
  readonly a: number;
}

function main(): void {
  const r: Record1 = { id: 1, xs: [1] };
  let d: Declared1 = { id: 0, xs: [] };
  d = r;
  d.xs.push(2);

  const r2: { readonly id: number; xs: number[] } = { id: 1, xs: [1] };
  let o: { readonly xs: number[]; id: number } = { id: 0, xs: [] };
  o = r2;
  o.xs = [5];

  let s: Set<readonly number[]> = new Set<readonly number[]>();
  s = new Set<number[]>([[1]]);
  s.forEach((x) => {
    x.push(1);
  });

  const shared: number[] = [1];
  let g: Getter | null = null;
  g = { get: (): number[] => shared };
  g.get().push(2);

  let c: Chain<number> = { v: 1, next: null };
  c = { v: 2, next: { v: [1], next: null } };
  const n = c.next;
  if (n !== null) {
    n.v.push(9);
  }

  let t: Tree | null = null;
  t = { val: 1, kids: [] };
  t.val = 2;

  let outer: Outer | null = null;
  outer = { inner: { xs: [1], next: null } };
  outer.inner.xs.push(2);

  let base: Base | { readonly x: number } | null = null;
  base = new Sub();
  base.x = 5;

  let callback: Callback | null = null;
  callback = { cb: (): number => 1 };
  callback.cb = (): number => 2;

  const plain: Mutable = { a: 1 };
  let either: Mutable | Frozen | null = null;
  either = plain;
  either.a = 5;

  let handler: Handler | null = null;
  handler = new Runner();
  handler.f = (): number => 5;

  const tagged: { f: (n: number) => number; readonly tag: number } = {
    f: (n: number): number => n,
    tag: 1,
  };
  let wrapped: Wrapped = { f: (n: number): number => n + 1, tag: 0 };
  wrapped = tagged;
  wrapped.f = (n: number): number => n * 2;

  let gadget: Gadget | null = null;
  gadget = new Widget();
  gadget.f = (): number => 5;

  let ids: WithExtra | IdOnly | string = "s";
  ids = { id: 1 };
  ids.extra = 5;

  let nested: Tight | Roomy | string = "s";
  nested = { id: 1, p: { x: 1 } };
  nested.p.y = 41;

  const wideList: readonly (Wide | null)[] = [{ x: 1 }];
  let lists: readonly (Wide | null)[] | readonly (Narrow | null)[] = wideList;
  const narrowList: readonly (Narrow | null)[] = [{ x: 3 }];
  lists = narrowList;
  const first = lists[0];
  if (first !== null) {
    first.y = 4;
  }

  let covered: Covered | Uncovered = { id: 0, x: { k: 1 } };
  covered = { id: 1, x: { k: 1 } };
  covered.x.s = 1;
}
