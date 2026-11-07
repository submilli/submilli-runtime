function sum<T extends { id: number }>(...values: T[]): number {
  let total = 0;
  for (const value of values) { total += value.id; }
  return total;
}
function first<T extends string>(value: T, ...rest: T[]): T { return value; }
class Helpers {
  static sum<T extends { id: number }>(...values: T[]): number {
    return sum(...values);
  }
}
function main(): void {
  const pair: [{ id: number; label: string }, { id: number; label: string }] = [
    { id: 20, label: "a" }, { id: 22, label: "b" }
  ];
  assert(sum(...pair) === 42, "constrained tuple spread");
  assert(Helpers.sum(...pair) === 42, "constrained static method spread");
  const rest = ["b", "c"];
  assert(first("a", ...rest) === "a", "constrained rest inference");
}
