// A `Call` step in an optional chain binds its arguments to the resolved
// signature, exactly like the non-chain method path: omitted defaulted
// params are synthesized and a variadic tail is packed into one array, so
// the argument list codegen sees lines up 1:1 with the callee's params.
// Without that, `a?.join()` reached a fixed-arity wrapper one argument
// short and `a?.concat(x)` handed the rest slot an unpacked value.

type Adder = (a: number, b: number) => number;

interface Greeter {
  greet(name: string, punctuation: string): string;
}

// A function-typed *property*, not a method: the step carries only the
// structural `Type::Function`, so binding falls back to that.
interface Formatter {
  fmt: (n: number) => string;
}

class Joiner {
  constructor(private sep: string) {}

  // A defaulted trailing param, omitted at the chain call site.
  wrap(parts: string[], prefix: string = "<"): string {
    return prefix + parts.join(this.sep);
  }

  // A rest tail, which the call site packs into one array.
  count(label: string, ...items: number[]): string {
    return label + ":" + items.length.toString();
  }
}

function joined(a: string[] | null): string | undefined {
  return a?.join();
}

function split(s: string | null): string[] | undefined {
  return s?.split(",");
}

function concatenated(a: number[] | null): number[] | undefined {
  return a?.concat([3, 4]);
}

function sorted(a: number[] | null): number[] | undefined {
  return a?.sort();
}

function wrapped(j: Joiner | null, parts: string[]): string | undefined {
  return j?.wrap(parts);
}

function counted(j: Joiner | null): string | undefined {
  return j?.count("n", 1, 2, 3);
}

function greeted(g: Greeter | null): string | undefined {
  return g?.greet("world", "!");
}

function called(f: Adder | null): number | undefined {
  return f?.(2, 3);
}

function formatted(f: Formatter | null): string | undefined {
  return f?.fmt(3);
}

function main(): void {
  assert(joined(["a", "b"]) === "a,b", "join's defaulted separator");
  assert(joined(null) === undefined, "null receiver short-circuits");

  const parts = split("a,b,c");
  assert(parts !== undefined && parts.length === 3, "split's defaulted limit");
  assert(split(null) === undefined, "null receiver short-circuits");

  const c = concatenated([1, 2]);
  assert(c !== undefined && c.join(",") === "1,2,3,4", "concat's rest tail packed");

  const s = sorted([3, 1, 2]);
  assert(s !== undefined && s.join(",") === "1,2,3", "sort's defaulted comparator");

  const j = new Joiner("-");
  assert(wrapped(j, ["a", "b"]) === "<a-b", "user default synthesized");
  assert(counted(j) === "n:3", "user rest tail packed");
  assert(counted(null) === undefined, "null receiver short-circuits");

  assert(greeted(null) === undefined, "interface method on a null receiver");

  assert(called((a: number, b: number): number => a + b) === 5, "closure call");
  assert(called(null) === undefined, "null closure short-circuits");

  const f: Formatter = { fmt: (n: number): string => "n=" + n.toString() };
  assert(formatted(f) === "n=3", "function-typed property called in a chain");
  assert(formatted(null) === undefined, "null receiver short-circuits");
}
