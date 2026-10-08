// A generic call in a literal argument's field is inferred early, without
// binding its own type parameters from its later arguments, only to bind the
// outer call's; what fails to type there binds nothing, and the call is
// inferred again for real.
function inner<U>(o: { g: (u: U) => U }, seed: U): { v: U } {
  return { v: o.g(seed) };
}

function outer<T>(o: { init: T; cb: (t: T) => string }): string {
  return o.cb(o.init);
}

function run<T, U>(o: { input: T; f: (t: T) => U }, seed: U): U {
  return o.f(o.input);
}

function seeded<T>(o: { seed: T; cb: (t: T) => string }): string {
  return o.cb(o.seed);
}

function apply<A, U>(o: { g: (a: A) => U }, seed: A): U {
  return o.g(seed);
}

function made<T>(o: { make: (n: number) => T; use: (t: T) => string }, seed: T): string {
  return o.use(o.make(1)) + o.use(seed);
}

function split<T>(o: { make: (n: number) => T }, p: { seed: T; use: (t: T) => string }): string {
  return p.use(o.make(1)) + p.use(p.seed);
}

interface Pipe<U> {
  make: (n: number) => U;
}

function piped<U>(o: Pipe<U>, fallback: U): U {
  return o.make(1);
}

function sided<T>(o: { cb: (t: T) => string }, side: number, seed: T): string {
  return o.cb(seed);
}

function partly<T>(o: { use: (n: number, t: T) => string; make: (n: number) => T }, seed: T): string {
  return o.use(1, o.make(1)) + o.use(2, seed);
}

function either<T>(
  o: { make: (n: number) => T } | { other: (n: number) => T },
  p: { use: (t: T) => string },
  seed: T,
): string {
  return p.use(seed);
}

function maybeUsed<T>(o: { use: ((n: number, t: T) => string) | null; make: (n: number) => T }, seed: T): string {
  return (o.use ? o.use(1, o.make(1)) : "") + String(seed);
}

interface Boxed<T> {
  value: T;
}

function unboxed<T>(o: { make: (n: number) => Boxed<T>; use: (t: T) => string }, seed: T): string {
  return o.use(o.make(1).value) + o.use(seed);
}

function shaped<T>(o: { use: (t: T) => string } | { x: number }, p: { use2: (t: T) => string }, seed: T): string {
  return p.use2(seed);
}

function curried<T>(o: { f: (n: number) => (u: T) => string }, seed: T): string {
  return o.f(1)(seed);
}

function ab(u: "a" | "b"): string {
  return u + "!";
}

function main(): void {
  const shown = outer({
    init: inner({ g: (u) => ({ name: u.name + "!" }) }, { name: "a" }),
    cb: (t) => t.v.name,
  });
  assert(shown === "a!", "the nested call's callback typed from its seed");
  // A later argument only a callback's result also types is a candidate
  // beside that result, not a binding the result must fit.
  assert(run({ input: 1, f: (t) => [t] }, [])[0] === 1, "an empty array seed");
  assert(run({ input: 1, f: (t) => t + 1 }, null) === 2, "a null seed");
  // A spread part holding such a call binds nothing early either.
  const spread = seeded({
    ...{ seed: apply({ g: (a) => ({ w: a.length }) }, "abcd") },
    cb: (t) => String(t.w),
  });
  assert(spread === "4", "a spread holding a nested generic call");
  // A callback whose result gives the type parameter before the first one
  // that reads it adds its candidate before a later argument does.
  assert(made({ make: (n) => n as number | null, use: (t) => String(t) }, 7) === "17", "a result before a read");
  assert(split({ make: (n) => n as number | null }, { seed: 7, use: (t) => String(t) }) === "17", "a seed in a later literal");
  assert(piped({ make: (n) => [n] }, [])[0] === 1, "a callback in an interface");
  // A callback that leaves out the parameter typed by `T` does not read it.
  assert(partly({ use: (n) => String(n), make: (n) => n as number | null }, 7) === "12", "a parameter left out");
  // A callback in a slot of no one shape may give `T` from its result.
  assert(either({ make: (n) => n as number | null }, { use: (t) => String(t) }, 7) === "7", "a union slot");
  // An argument that assigns stops the early binding, so the one after it
  // sees the assignment.
  let v: number | string = "ab";
  assert(sided({ cb: (t) => String(t) }, (v = 5), v) === "5", "an assignment before the seed");
  assert(maybeUsed({ use: (n) => String(n), make: (n) => n as number | null }, 7) === "17", "a nullable callback slot");
  // A callback returning a generic type that holds `T` does not read it.
  assert(unboxed({ make: (n) => ({ value: n as number | null }), use: (t) => String(t) }, 7) === "17", "a returned box");
  // A field of some members of a union shape is typed by those members.
  assert(shaped({ use: (t) => t.name }, { use2: (t) => t.name }, { name: "x" }) === "x", "a union shape");
  // A callback returning one reads `T` through that one's parameter.
  assert(curried({ f: (n) => (u) => u.toFixed(n) }, 1.5) === "1.5", "a curried callback");
  // A returned named function, or one annotating its parameter, gives `T`
  // from its type instead.
  assert(curried({ f: (n) => ab }, "a") === "a!", "a returned named function");
  assert(curried({ f: (n) => (u: "a" | "b") => u + "#" }, "a") === "a#", "a returned annotated function");
}
