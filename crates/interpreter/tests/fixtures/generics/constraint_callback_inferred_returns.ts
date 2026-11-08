type A = { kind: "a"; x: number };
function mk<F extends (n: number) => number>(read: F): number { return read(1); }
function object<F extends (n: number) => A>(read: F): A { return read(1); }
function inferred<R>(read: (n: number) => R): R { return read(1); }
function discard<F extends (n: number) => void>(read: F): void { read(1); }
function keep<F extends (n: number) => number>(read: F): F { return read; }
function keepObject<F extends (n: number) => A>(read: F): F { return read; }

function main(): void {
  assert(mk(n => { return n + 1; }) === 2, "block infers arithmetic return");
  assert(mk(n => { return 5; }) === 5, "block infers constant return");
  assert(mk(function (n) { return n + 1; }) === 2, "function expression infers return");
  assert(object(n => { return { kind: "a", x: n + 1 }; }).x === 2, "block keeps literal-field return context");
  assert(object(function (n) { return { kind: "a", x: n + 2 }; }).x === 3, "function expression keeps literal fields");
  assert(mk(n => n + 1) === 2, "expression callback");
  assert(mk((n: number) => { return n + 1; }) === 2, "annotated parameter");
  assert(mk(() => { return 5; }) === 5, "omitted parameter");
  assert(inferred(n => { return n + 1; }) === 2, "unconstrained return inference");
  const callback: (n: number) => number = keep(n => { return n + 1; });
  assert(callback(1) === 2, "inferred return survives returning the callback");
  const withExtra = keepObject(n => { return { kind: "a", x: n, extra: "kept" }; });
  assert(withExtra(1).extra === "kept", "inferred object return retains extra fields");
  let seen = 0;
  discard(n => { seen = n; });
  assert(seen === 1, "void bound");
  discard(n => { seen = n + 1; return n + 2; });
  assert(seen === 2, "void bound discards an inferred value");
}
