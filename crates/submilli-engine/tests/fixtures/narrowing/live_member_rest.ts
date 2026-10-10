class Joiner { concat(first: number[], second: number[]): number[] { return first; } }
let value: number[] | Joiner = [1];
function change(): boolean { value = new Joiner(); return false; }
function read(): number[] {
  if (!Array.isArray(value) || change()) return [];
  return value.concat([2], [3]);
}
function main(): void { assert(read()[0] === 2); }
