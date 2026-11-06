// A constructor assigned to an interface with a call signature is a function:
// calling it converts as the constructor's call signature does, as in
// TypeScript. An omitted argument takes the signature's default, so `Number()`
// is 0 while `Number(undefined)` is NaN, and `String()` is "" while
// `String(undefined)` is "undefined".
interface FromNum {
  (v: number): number;
}
interface Zero {
  (): number;
}
interface OptConv {
  (v?: number | string): number;
}
interface ToStr {
  (v: unknown): string;
}
interface OptStr {
  (v?: string): string;
}
interface ToBool {
  (v?: unknown): boolean;
}
interface ToBig {
  (v: string | number): bigint;
}
interface Generic<T> {
  (v: T): number;
}
function apply(f: FromNum, v: number): number {
  return f(v);
}
function callEmpty(f: OptStr): string {
  return f();
}
function main(): void {
  const f: FromNum = Number;
  const z: Zero = Number;
  const o: OptConv = Number;
  assert(f(3) === 3 && z() === 0 && o() === 0 && isNaN(o(undefined)) && o("7") === 7);
  assert(apply(Number, 4) === 4);
  const s: ToStr = String;
  assert(s(12) === "12" && s(null) === "null" && s(undefined) === "undefined" && s([1, 2]) === "1,2");
  const os: OptStr = String;
  assert(os() === "" && callEmpty(String) === "" && os(undefined) === "undefined" && os("x") === "x");
  assert(String() === "" && String(undefined) === "undefined");
  const b: ToBool = Boolean;
  assert(!b() && !b(0) && b("x"));
  const g: ToBig = BigInt;
  assert(g("12") === 12n && g(5) === 5n);
  let threw = false;
  try {
    g("x");
  } catch (e) {
    threw = e instanceof SyntaxError;
  }
  assert(threw);
  // A value the constructor refuses, reachable only through a cast, throws a
  // catchable TypeError, as in JavaScript.
  const big: unknown = BigInt;
  for (const value of [null, undefined]) {
    let refused = false;
    try {
      (big as (v: unknown) => bigint)(value);
    } catch (e) {
      refused = e instanceof TypeError;
    }
    assert(refused);
  }
  const generic: Generic<string> = Number;
  assert(generic("8") === 8);
  assert(f === Number && `${f}` === "function Number() { [native code] }" && typeof f === "function");
  const fns: FromNum[] = [Number, Number];
  assert(fns.map((h) => h(2)).join(",") === "2,2");
}
