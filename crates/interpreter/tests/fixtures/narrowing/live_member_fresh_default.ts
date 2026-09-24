class Joiner {
  concat(first: number[] = []): number[] { first.push(1); return first; }
}
let value: number[] | Joiner = [1];
function change(): boolean { value = new Joiner(); return false; }
function main(): void {
  if (!Array.isArray(value) || change()) return;
  assert(value.concat().length === 1);
  assert(value?.concat().length === 1);
}
