const isNumber = (x: number | string): x is number => typeof x === "number";
const isString = (x: number | string): x is string => { return typeof x === "string"; };
function read(x: number | string): string {
  if (isNumber(x)) { return x.toString(); }
  return x.toUpperCase();
}
function second(ignored: boolean, x: string | null): string {
  const present = (flag: boolean, value: string | null): value is string => value !== null;
  if (present(ignored, x)) { return x.toUpperCase(); }
  return "nil";
}
function block(x: number | string): number {
  if (isString(x)) { return x.length; }
  return x + 1;
}
export function main(): void {
  assert(read(3) === "3", "expression arrow true branch");
  assert(read("ok") === "OK", "expression arrow false branch");
  assert(second(false, "yes") === "YES", "predicate indexes its own parameter");
  assert(second(true, null) === "nil", "local arrow false branch");
  assert(block("abc") === 3, "block arrow true branch");
  assert(block(3) === 4, "block arrow false branch");
}
