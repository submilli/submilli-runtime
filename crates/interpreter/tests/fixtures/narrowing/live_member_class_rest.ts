class Joiner { concat(...parts: number[][]): number[] { return parts[0]; } }
let value: number[] | Joiner = [1];
function change(): boolean { value = new Joiner(); return false; }
function main(): void {
  if (!Array.isArray(value) || change()) return;
  assert(value.concat([2], [3])[0] === 2);
}
