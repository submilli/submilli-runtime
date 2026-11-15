// expect-error-count: 1
// expect-error: cannot read field `join`
// A call in the condition invalidates the preceding module-variable guard.
class Joiner { join(separator: string = "custom"): string { return separator; } }
let value: number[] | Joiner = [1];
function change(): boolean { value = new Joiner(); return false; }
function read(): string {
  if (!Array.isArray(value) || change()) return "fallback";
  return value.join();
}
function main(): void { assert(read() === "custom"); }
