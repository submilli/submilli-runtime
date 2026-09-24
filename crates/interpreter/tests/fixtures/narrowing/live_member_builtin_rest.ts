class Joiner { concat(first: number[], second: number[]): number[] { return first; } }
let value: number[] | Joiner = new Joiner();
function change(): boolean { value = [1]; return false; }
function main(): void {
  if (!(value instanceof Joiner) || change()) return;
  assert(JSON.stringify(value.concat([2], [3])) === "[1,2,3]");
}
