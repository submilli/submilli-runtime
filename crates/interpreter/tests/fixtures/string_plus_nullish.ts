let order = "";
class OrderedError extends Error {
  toString(): string { order += "convert;"; return "error"; }
}
function right(): string { order += "right;"; return "!"; }
function ordered(value: OrderedError | null): string { return value + right(); }
function orderedCompound(value: OrderedError | string | null): string {
  value += right();
  return String(value);
}
enum Code { Ready = 7 }
function check(value: string | number | bigint | boolean | Code | Error | null | undefined, expected: string): void {
  assert("x" + value === "x" + expected);
  assert(value + "y" === expected + "y");
  let joined = "x";
  joined += value;
  assert(joined === "x" + expected);
}
function absent(): undefined { console.log("absent"); return undefined; }
function main(): void {
  assert(ordered(new OrderedError("")) === "error!");
  assert(order === "right;convert;");
  order = "";
  assert(orderedCompound(new OrderedError("")) === "error!");
  assert(order === "right;convert;");
  check(undefined, "undefined");
  check(null, "null");
  check("a", "a");
  check(12, "12");
  check(13n, "13");
  check(true, "true");
  check(Code.Ready, "7");
  check(new Error("bad"), "Error: bad");
  assert("x" + null === "xnull");
  assert(undefined + "y" === "undefinedy");
  assert("x" + absent() === "xundefined");
  const match = "a".match(/a(b)?/);
  if (match !== null) {
    const group = match[1] ?? undefined;
    assert("group:" + group === "group:undefined");
  }
}
