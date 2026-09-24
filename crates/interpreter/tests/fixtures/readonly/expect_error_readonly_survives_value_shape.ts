// A write narrows a binding with `readonly` in its declaration to the declared
// member, so the declaration's `readonly` survives wherever the value's type
// puts its own: in a different field, inside a type argument, in a function
// type's return, or inside a recursive alias. Each of the six writes below
// matches a `tsc --strict` error.
// expect-error-count: 6
// expect-error: cannot call `push` on `readonly number[]`
// expect-error: cannot assign to readonly property `xs`
// expect-error: cannot assign to readonly property `val`

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
}
