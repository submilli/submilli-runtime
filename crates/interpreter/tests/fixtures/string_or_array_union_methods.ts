// A method both strings and arrays declare is called on their union as tsc
// calls a union's method: each argument fits both signatures, and the value's
// own method runs. The receiver and each argument are evaluated once.
type Tag = "a" | "b";
let calls = 0;
function pick(n: number): string | number[] {
  calls++;
  return n > 0 ? "hello" : [1, 2, 3];
}
function arg(n: number): number {
  calls++;
  return n;
}
function main(): void {
  const s = pick(1).slice(arg(1), arg(3));
  assert(s === "el", "a string slices as a string");
  const a = pick(0).slice(arg(1));
  assert(a.length === 2 && typeof a !== "string" && a[0] === 2, "an array slices as an array");
  assert(calls === 5, "receiver and arguments are evaluated once");

  const words: string | string[] = calls > 0 ? ["x", "y"] : "xy";
  assert(words.indexOf("y") === 1, "indexOf on the array");
  assert(words.includes("x"), "includes on the array");
  assert(words.at(-1) === "y", "at on the array");
  assert(words.toString() === "x,y", "toString on the array");
  const text: string | string[] = calls > 0 ? "qa" : ["q"];
  assert(text.indexOf("a") === 1 && text.lastIndexOf("q") === 0, "indexOf on the string");

  // A literal argument keeps its type, which the array's elements need.
  const tags: string | Tag[] = calls > 0 ? ["a", "b"] : "ab";
  assert(tags.includes("b") && tags.indexOf("a") === 0, "a literal fits the element type");
}
