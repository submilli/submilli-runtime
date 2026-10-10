// A literal that is a source for inference, so not checked for unknown
// fields, is still rejected when it shares no field with a target type whose
// fields are all optional and no other member of the target takes it, as in
// tsc. Fields a spread copies count.
// expect-error: object literal has no fields in common with `Weak`, whose fields are all optional
// expect-error: object literal has no fields in common with `{ a?: number; b?: string }`
// expect-error-count: 5
type Weak = { a?: number; b?: string };
type B = { b: string };

function id<T>(x: T): T {
  return x;
}

function list<T>(xs: T[]): T[] {
  return xs;
}

function make(f: () => Weak | B): Weak | B {
  return f();
}

function main(): void {
  const q = { q: 1 };
  const direct: Weak = id({ q: 1 });
  const union: Weak | B = id({ q: 1 });
  const elements: (Weak | B)[] = list([{ q: 1 }]);
  const returned = make(() => ({ q: 1 }));
  const spread: Weak = id({ ...q, z: 1 });
}
