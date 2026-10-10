let calls: number = 0;
class Value {
  valueOf(): number { calls += 1; return 7; }
  toString(): string { calls += 10; return "wrong"; }
}
let value: number | Value = 3;
function change(): boolean { value = new Value(); return false; }
function result(): number {
  if (typeof value !== "number" || change()) return 0;
  return value + 1;
}
function main(): void { assert(result() === 8); assert(calls === 1); }
