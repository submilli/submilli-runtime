interface Callable { (a: number, b: number): number; length?: number }
interface Unary { (x: number): number; length?: number }
function eraseLength(o: { length?: number }): boolean { return delete o.length; }
function lengthOf(o: { length?: number }): number | undefined { return o.length; }
function eraseIndex(o: { index?: number }): boolean { return delete o.index; }
function eraseInput(o: { input?: string }): boolean { return delete o.input; }
function eraseGroups(o: { groups?: unknown }): boolean { return delete o.groups; }
function identity<T>(x: T): T { return x; }
function optionalIndex(m: RegExpExecArray | null): unknown { return m?.index; }
function optionalInputLength(m: RegExpExecArray | null): unknown { return m?.input?.length; }
function missing(value: unknown): boolean { return value === undefined; }
function absent(o: object, key: string): boolean { return !(key in o) && !Object.hasOwn(o, key); }
function main(): number {
  const f: Callable = (a: number, b: number): number => a + b;
  const alias = f;
  if (!eraseLength(f) || f.length !== 0 || alias.length !== 0) return 1;
  if (Object.hasOwn(f, "length") || !("length" in f) || f(2, 3) !== 5) return 2;
  if (!eraseLength(f)) return 3;
  const specialized: (x: number) => number = identity;
  const poly: Unary = specialized;
  const bound: (x: number) => number = identity;
  if (!eraseLength(poly) || bound.length !== 0 || bound(7) !== 7) return 4;
  const underlying = (n: number): number => n + 1;
  const hybrid: Unary = Object.assign(underlying, { label: "callable" });
  if (!eraseLength(hybrid) || hybrid.length !== 0 || underlying.length !== 0) return 14;
  if (Object.hasOwn(hybrid, "length") || hybrid(2) !== 3 || lengthOf(hybrid) !== 0) return 15;
  const original = (): number => 42;
  const adapted: (x: number) => number = original;
  const functions = new Map<object, number>();
  functions.set(original, 9);
  if (functions.get(adapted) !== 9) return 12;
  const set = new Set<object>();
  set.add(adapted);
  if (!set.has(original)) return 13;
  const m = /(?<a>a)/.exec("a");
  if (m === null) return 5;
  if (!eraseIndex(m) || !missing(m.index) || !absent(m, "index") || !missing(optionalIndex(m))) return 6;
  if (!eraseInput(m) || !missing(m.input) || !absent(m, "input") || !missing(optionalInputLength(m))) return 7;
  if (!eraseGroups(m) || !missing(m.groups) || !absent(m, "groups")) return 8;
  if (Object.keys(m).join(",") !== "0,1") return 9;
  if ("index" in { ...m } || "input" in { ...m } || "groups" in { ...m }) return 10;
  if (JSON.stringify(m) !== '["a","a"]') return 11;
  return 0;
}
