// A literal passed straight for a type parameter that is the call's result
// keeps its literal type, as in tsc: `id(1)` is `1`. So does the return of a
// function literal passed for a bare type parameter: `id(() => 42)` is
// `() => 42` when it is the type parameter's only candidate. A `let` still
// widens what it copies.
function id<T>(x: T): T {
  return x;
}

function maybe<T>(x: T, keep: boolean): T | null {
  return keep ? x : null;
}

type Mode = "on" | "off";

class Holder<T> {
  constructor(public value: T) {}
}

function unbox<T>(holder: Holder<T>): T {
  return holder.value;
}

function field<T>(o: { value: T }): T {
  return o.value;
}

function call<T>(f: () => T): T {
  return f();
}

function last<T>(...xs: T[]): T {
  return xs[xs.length - 1];
}

function second<A, B>(a: A, b: B): B {
  return b;
}

function whichever<L, R>(left: L, right: R): L | R {
  return right;
}

function orMode<B>(mode: Mode, value: B): B | Mode {
  return value;
}

function orList<T, E>(value: T, fallback: E[]): T | E[] {
  return value;
}

type Opt<A> = A | null;
type Id<A> = A;

function orNull<A>(value: A): Opt<A> {
  return value;
}

function same<A>(value: A): Id<A> {
  return value;
}

function oneOrMany<T>(many: T[], one: T): T | T[] {
  return one;
}

function head<T>(pair: [T, number]): T {
  return pair[0];
}

function first<T>(xs: T[]): T {
  return xs[0];
}

function pick<T>(first: T, second: T): T {
  return second;
}

function main(): void {
  const one: 1 = id(1);
  const word: "s" = id("s");
  const yes: true = id(true);
  const kept: "k" | null = maybe("k", true);
  assert(one === 1 && word === "s" && yes && kept === "k", "the literal types");

  const answer: () => 42 = id(() => 42);
  const branches: () => number = id(() => {
    if (one > 5) {
      return 1;
    }
    return 2;
  });
  assert(answer() === 42 && branches() === 2, "returned literals");

  // With two candidates neither keeps its literals, so they agree.
  const later = pick(() => "a", () => "b");
  const counts = pick((n: number) => 0, (n: number) => n + 1);
  assert(later() === "b" && counts(4) === 5, "two candidates widen");

  // Nor does a function literal in a conditional, whose branches would
  // otherwise disagree.
  const chosen = id(one > 5 ? () => 1 : () => 2);
  assert(chosen() === 2, "a conditional's branches widen");

  // A function literal's inferred return type widens a call's literal
  // result, as it widens a literal.
  let letter = () => id("a");
  letter = () => "b";
  const lengths = [1, 2].map(() => id(0));
  lengths.push(4);
  assert(letter() === "b" && lengths.join(",") === "0,0,4", "a returned call widens");

  // So does a fresh literal reached through a `const`, whether the `const`
  // holds the literal or a call that kept it.
  const label = "fixed";
  const flag = true;
  const labels = [1, 2].map(() => label);
  labels.push("other");
  const flags = [1].map(() => flag);
  flags.push(false);
  const blocks = [1].map(() => {
    const local = "blk";
    return local;
  });
  blocks.push("more");
  const kept1 = id(1);
  let counter = () => kept1;
  counter = () => 2;
  assert(labels.join(",") === "fixed,fixed,other" && flags.join(",") === "true,false", "a const's literal widens");
  assert(blocks.join(",") === "blk,more" && counter() === 2, "through a block and a kept call");

  // A union of literals is no single literal, so a returned one stays.
  const either = one > 5 ? "on" : "off";
  const eithers: Mode[] = [1].map(() => either);
  assert(eithers[0] === "off", "a returned union stays");

  // A literal type the call declares, rather than takes from a fresh
  // argument, stays.
  const modes: Mode[] = ["on", "off"];
  const lookup = new Map<string, Mode>([["x", "off"]]);
  const firstMode = () => first(modes);
  const found = () => modes.find((mode) => mode === "off") ?? null;
  const stored = () => lookup.get("x") ?? null;
  const mode: Mode = firstMode();
  const maybeMode: Mode | null = found() ?? stored();
  const echoed: Mode[] = modes.map((each) => id(each));
  assert(mode === "on" && maybeMode === "off", "a declared literal type stays");
  assert(echoed.join(",") === "on,off", "through a call that keeps it");

  let copy = id(1);
  copy = 5;
  assert(copy === 5, "a let widens");

  // A fresh literal read back out of a generic container is still fresh, but
  // a written type argument declares it.
  const hello = "hello";
  let unboxed = unbox(new Holder(hello));
  unboxed = "x";
  let total = [1, 2].reduce((sum, n) => sum, hello);
  total = "y";
  const fixed = id<1>(1);
  const fixedLater = () => fixed;
  const declaredOne: 1 = fixedLater();
  assert(unboxed === "x" && total === "y" && declaredOne === 1, "a container's literal");

  // A declared literal passed through a callback, beside a fresh one, or
  // inside an object literal stays declared.
  let viaCallback = call(() => mode);
  let besideFresh = pick(mode, "on");
  let inObject = field({ value: mode });
  const declaredModes: Mode[] = [viaCallback, besideFresh, inObject];
  let rested = last<1>(1);
  const restedOne: 1 = rested;
  assert(declaredModes.join(",") === "on,on,on" && restedOne === 1, "declared through a call");

  // A declared literal for one type parameter leaves a fresh one for another
  // fresh, and a tuple or nested call carries its own declared literals.
  let other = second(mode, "on");
  other = "elsewhere";
  let paired = head([mode, 1]);
  let nested = field({ value: pick(mode, "on") });
  const carried: Mode[] = [paired, nested];
  assert(other === "elsewhere" && carried.join(",") === "on,on", "per type parameter");

  // A literal one type parameter of the result binds regular stays declared
  // however another binds it, as does one the callee's return type names.
  let joined = whichever(mode, "on");
  let orDeclared = orMode(mode, "on");
  const unioned: Mode[] = [joined, orDeclared];
  assert(unioned.join(",") === "on,on", "a declared literal in the result");

  // A declared literal nested in the result's array member doesn't absorb a
  // fresh one in its other member.
  let listed = orList("on", modes);
  listed = "elsewhere";
  assert(listed === "elsewhere", "a nested declared literal");

  // A fresh literal behind a generic alias still widens at a `let`, and one
  // beside a declared array's elements takes their declared type.
  let optional = orNull("on");
  optional = "elsewhere";
  let aliased = same(1);
  aliased = 2;
  let many = oneOrMany(modes, "on");
  const declaredMany: Mode | Mode[] = many;
  assert(optional === "elsewhere" && aliased === 2 && declaredMany === "on", "through an alias");
}
