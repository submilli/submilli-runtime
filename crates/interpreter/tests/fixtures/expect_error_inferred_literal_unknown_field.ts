// A literal assigned straight to a declared type is still checked for unknown
// fields where a type parameter is involved, as in tsc: one given as a type
// argument, the structure a parameter declares around one, a function
// literal's return annotation, and declarations inside a function literal.
// So is one inferred from anything but literals, wherever that argument is,
// and a union parameter's member with structure around it.
// expect-error: object literal has unknown field `z` for the target type
// expect-error: object literal has unknown field `y` for the target type
// expect-error: object literal has unknown field `x` for the target type
// expect-error: object literal has unknown field `w` for the target type
// expect-error: object literal has unknown field `v` for the target type
// expect-error: object literal has unknown field `u` for the target type
// expect-error: object literal has unknown field `t` for the target type
// expect-error: object literal has unknown field `r` for the target type
// expect-error-count: 8
type A = { a: number };

function id<T>(x: T): T {
  return x;
}

function wrap<T>(x: { v: T }): T {
  return x.v;
}

function both<T>(x: T, y: T): T {
  return y;
}

function boxedOrBare<T>(x: T | { v: T }): T | { v: T } {
  return x;
}

function make(f: () => A): A {
  return f();
}

function main(): void {
  const explicit: A = id<A>({ a: 1, z: 2 });
  const around: number = wrap({ v: 1, y: 2 });
  const annotatedReturn = make((): A => ({ a: 1, x: 2 }));
  const declared = make(() => {
    const inner: A = { a: 1, w: 2 };
    return inner;
  });
  const nestedFunction = make(() => {
    function inner(): A {
      return { a: 1, v: 2 };
    }
    return inner();
  });
  const call = make(() => ({ a: id<A>({ a: 1, u: 2 }).a }));
  const base: A = { a: 1 };
  const afterDeclared = both(base, { a: 2, t: 3 });
  const structured = boxedOrBare({ v: 1, r: 2 });
}
