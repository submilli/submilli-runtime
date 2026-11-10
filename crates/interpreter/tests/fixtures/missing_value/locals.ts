// SUB-2148: an unannotated local initialized by an index read holds it maybe
// missing. Each use is decided as the read would be, and a `return` reads it:
// a maybe-missing value never leaves its function, or enters a function type,
// a field, an array element or a generic argument.
function identity<T>(x: T): T {
  return x;
}

function returned(r: Record<string, string>): string {
  const v = r["k"];
  return v;
}

function returnedOrNone(r: Record<string, string>): string | undefined {
  const v = r["k"];
  return v;
}

function message(read: () => string): string {
  try {
    return "=" + read();
  } catch (e: MissingValueError) {
    return e.message;
  }
}

function main(): void {
  const r: Record<string, string> = { a: "A" };

  assert(message(() => returned(r)) === "`v` is missing: `r[\"k\"]` on line 10 found no value", "return from a `string` function");
  assert(returnedOrNone(r) === undefined, "return from a `string | undefined` function");
  assert(message(() => { const v = r["k"]; const w = v; return w ?? "w0"; }) === "=w0", "a copy stays maybe missing");
  assert(message(() => { let v = r["k"]; v = "set"; return v; }) === "=set", "assigning a value makes it present");
  assert(message(() => { let v = r["k"]; v = r["a"]; return v; }) === "=A", "assigning another read");
  assert(message(() => { const v = r["a"]; return v + "!"; }) === "=A!", "a present value reads as itself");
  assert(
    message(() => { const v = r["k"]; const f = (): string => v; return f(); }) === "key \"k\" is missing from `r`; use `r[key] ?? …` or check `key in r` first",
    "a captured local throws at its read",
  );
  assert(message(() => { const f = (): string => r["k"]; return f() ?? "x"; }).startsWith("key \"k\""), "an arrow returning the read");
  assert(message(() => { const v = r["k"]; const o = { v }; return o.v; }).startsWith("`v` is missing"), "an object literal");
  assert(message(() => { const v = r["k"]; return [v].join(); }).startsWith("`v` is missing"), "an array literal");
  assert(message(() => { const v = r["k"]; return identity(v); }).startsWith("`v` is missing"), "a generic argument");
  assert(message(() => { const xs: number[] = []; const first = xs[0]; return String(first * 2); }).startsWith("`first` is missing"), "arithmetic reads it");
}
