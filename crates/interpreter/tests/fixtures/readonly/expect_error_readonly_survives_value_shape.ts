// A write narrows a binding with `readonly` in its declaration to the declared
// member, so the declaration's `readonly` survives wherever the value's type
// puts its own: in a different field, inside a type argument, in a function
// type's return, inside a recursive or mutually recursive alias, or in a
// function-valued interface property. A value's own `readonly` survives too: a
// subclass that makes an inherited field `readonly` keeps it, and of two members
// differing only in a field's `readonly` neither stands for the value alone.
// Each of the ten writes below matches a `tsc --strict` error.
// expect-error-count: 10
// expect-error: cannot call `push` on `readonly number[]`
// expect-error: cannot assign to readonly property `xs`
// expect-error: cannot assign to readonly property `val`
// expect-error: cannot assign to readonly field `x` on `Sub`
// expect-error: cannot assign to readonly property `cb`

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
}
