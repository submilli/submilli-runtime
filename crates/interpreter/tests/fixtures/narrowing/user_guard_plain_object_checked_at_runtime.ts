// A guard on untyped data whose body narrows only to `object` checks the rest
// of its object predicate at run time, as `as` would: plain data behaves as in
// tsc, and a value that isn't of the shape throws a `TypeError` when the body
// returns true.
type Rec = { [k: string]: unknown };
interface User { name: string }
function isRec(v: unknown): v is Rec {
  return typeof v === "object" && v !== null && !Array.isArray(v);
}
const isRecArrow = (v: unknown): v is Rec => typeof v === "object" && v !== null;
function isUser(v: unknown): v is User {
  if (typeof v !== "object" || v === null) return false;
  return "name" in v;
}
function isRecOrNull(v: object | null): v is Rec {
  return v !== null;
}
function isAnyObject(v: unknown): v is Record<string, unknown> {
  return typeof v === "object" && v !== null;
}
type Shape = { kind: "circle"; radius: number } | { kind: "rect"; width: number };
function isShape(v: unknown): v is Shape {
  return typeof v === "object" && v !== null && "kind" in v;
}
function isCapturedRec(v: unknown): v is Rec {
  const read = (): unknown => v;
  if (typeof read() === "string") return false;
  return typeof v === "object" && v !== null;
}
function hasAOrB(v: unknown): v is { a: number } | { b: string } {
  return typeof v === "object" && v !== null && ("a" in v || "b" in v);
}
function isBox<T>(v: unknown, tag: T): v is { value: number } {
  return tag !== null && typeof v === "object" && v !== null;
}
function describe(v: unknown): string {
  if (isShape(v)) {
    return v.kind;
  }
  return "other";
}
function throwsTypeError(f: () => boolean): boolean {
  try {
    f();
    return false;
  } catch (e) {
    return e instanceof TypeError;
  }
}

export function main(): void {
  const raw: unknown = JSON.parse('[{"a":1},"s",[1],null,{"b":{"c":2}}]');
  const items: unknown[] = Array.isArray(raw) ? raw : [];
  const recs: Rec[] = items.filter(isRec);
  assert(recs.length === 2, "filter keeps the plain objects");
  assert(recs[0]?.["a"] === 1, "a kept record reads its fields");
  assert(isRecArrow({}), "arrow guard accepts a plain object");
  assert(!isRec("s"), "a string is not a record");
  assert(throwsTypeError(() => isRec(new Map<string, number>())), "a Map throws");
  assert(isUser(JSON.parse('{"name":"a"}')), "a user");
  assert(!isUser(JSON.parse('{"x":1}')), "no name field");
  assert(throwsTypeError(() => isUser(JSON.parse('{"name":1}'))), "a non-string name throws");
  assert(isRecOrNull({ a: 1 }) && !isRecOrNull(null), "an `object | null` parameter");
  assert(throwsTypeError(() => isAnyObject([1, 2])), "an array is an object but not a record");
  assert(describe(JSON.parse('{"kind":"rect","width":2}')) === "rect", "a union of shapes");
  assert(describe(JSON.parse('{"other":1}')) === "other", "no discriminant");
  assert(throwsTypeError(() => isShape(JSON.parse('{"kind":"line"}'))), "an unknown discriminant throws");
  assert(isCapturedRec({}) && !isCapturedRec(1), "a parameter a closure captures");
  assert(hasAOrB(JSON.parse('{"b":"x"}')) && !hasAOrB(JSON.parse("{}")), "a union of `in` shapes");
  assert(throwsTypeError(() => hasAOrB(JSON.parse('{"a":"x"}'))), "a field of the wrong type throws");
  assert(isBox(JSON.parse('{"value":1}'), 0), "a generic guard");
  assert(throwsTypeError(() => isBox(JSON.parse('{"value":"x"}'), 0)), "a generic guard throws");
}
