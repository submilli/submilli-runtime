// A generic function value becomes a function type without type parameters
// where it is assigned, passed, returned or cast: its type arguments are
// inferred there and bound once, so its run-time tests of them check what the
// target's types allow.
import session from "submilli:session";

function rejects(run: () => void): boolean {
  try {
    run();
  } catch (e: TypeError) {
    return e.message.indexOf("does not match") >= 0;
  }
  return false;
}

const load = <T,>(key: string): T | undefined => session.get<T>(key);

function collect<T>(accept: (v: T) => void, out: T[]): void {
  const v = session.get<T>("str");
  if (v !== undefined) {
    out.push(v);
  }
}

const first = <T,>(out: T[]): T | undefined => {
  const v = session.get<T>("num");
  if (v !== undefined) {
    out.push(v);
  }
  return v;
};

const bounded = <T, U extends T>(key: string, t: T, cb: (x: T) => void): void => {
  const v = session.get<U>(key);
  if (v !== undefined) {
    cb(v);
  }
};

const forward = <U extends T, T>(s: string): number => 1;
const fBounded = <T extends { next?: T }>(k: string): number => 7;

class Statics {
  static load: <T>(key: string) => T | undefined = load;
}

const readOpt = <T,>(key: string, fallback?: T): T | undefined => session.get<T>(key) ?? fallback;

function numbers(): (key: string) => number | undefined {
  return load;
}

function callWith(f: (key: string) => number | undefined, key: string): number | undefined {
  return f(key);
}

interface Plain {
  read(key: string): number | undefined;
}

interface Loader {
  load<T>(key: string): T | undefined;
}

class Many {
  pick<T>(a: T, b: T, c: T, d: T, e: T): T {
    return e;
  }
}

function main(): void {
  session.set("num", 1);
  session.set("str", "x");

  const n: (key: string) => number | undefined = load;
  assert(n("num") === 1, "an initializer");
  assert(rejects(() => { n("str"); }), "checks the target's type");

  assert(callWith(load, "num") === 1, "an argument");
  assert(rejects(() => { callWith(load, "str"); }), "checked");

  assert(numbers()("num") === 1, "a return");
  assert(rejects(() => { numbers()("str"); }), "checked");

  const cast = load as (key: string) => number | undefined;
  assert(cast("num") === 1, "a cast");
  assert(rejects(() => { (load as (key: string) => string | undefined)("num"); }), "checked");

  const field: Plain = { read: load };
  assert(field.read("num") === 1, "an object literal's field with an expected type");
  assert(rejects(() => { field.read("str"); }), "checked");

  const listed: ((key: string) => number | undefined)[] = [load];
  assert(listed[0]("num") === 1, "an array literal's element with an expected type");
  assert(rejects(() => { listed[0]("str"); }), "checked");

  const mapped = ["num"].map<number | undefined>(load);
  assert(mapped[0] === 1, "a host callback");
  assert(rejects(() => { ["str"].map<number | undefined>(load); }), "checked");

  const ignore: (key: string) => void = load;
  ignore("str");
  assert(true, "a dropped result leaves the type parameter unconstrained");

  const nums: number[] = [];
  const c: (accept: (v: unknown) => void, out: number[]) => void = collect;
  assert(rejects(() => { c((v: unknown): void => {}, nums); }), "a plain parameter wins over a callback's");
  const g: (out: number[]) => unknown = first;
  assert(g(nums) === 1, "over the result");
  assert(nums.length === 1, "and the value reached the array");

  const b: (key: string, t: number, cb: (x: number) => void) => void = bounded;
  assert(rejects(() => { b("str", 1, (x: number): void => {}); }), "an uninferred bound takes the others' arguments");

  const fw: (s: string) => number = forward;
  assert(fw("x") === 1, "an uninferred bound naming a later parameter");
  const fb: (k: string) => number = fBounded;
  assert(fb("x") === 7, "an F-bounded parameter never inferred");

  assert(Statics.load<number>("num") === 1, "a static field of a generic function type");
  assert(rejects(() => { Statics.load<string>("num"); }), "takes the call's type arguments");

  const a: (key: string) => number | undefined = readOpt;
  assert(a("num") === 1, "an omitted optional parameter: the result tells");
  assert(rejects(() => { a("str"); }), "checked");

  const unknownReader: (key: string) => unknown = load;
  const again: <U>(key: string) => unknown = unknownReader;
  assert(again<string>("num") === 1, "an instantiated function stays instantiated");

  const loader: Loader = { load: load };
  assert(loader.load<number>("num") === 1, "a generic method through its own generic type");
  assert(rejects(() => { loader.load<string>("num"); }), "checked");
  const copy = { ...loader };
  assert(copy.load<number>("num") === 1, "a spread copy keeps it");

  const hidden = load as unknown as (key: string) => number | undefined;
  assert(rejects(() => { hidden("num"); }), "through `unknown`, its type arguments are poisoned");

  assert(new Many().pick(1, 2, 3, 4, 5) === 5, "a five-parameter generic method");
}
