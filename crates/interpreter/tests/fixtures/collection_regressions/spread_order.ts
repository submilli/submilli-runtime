let calls = "";
function value(): number { calls = calls + "v"; return 1; }
function source(): [number, number] { calls = calls + "s"; return [2, 3]; }
function change(a: number[]): number { a[0] = 9; return 4; }
function main(): void {
  const tuple: [number, number, number, number] = [value(), ...source(), value()];
  assert(calls === "vsv", "tuple evaluation order");
  calls = "";
  const array = [value(), ...source(), value()];
  assert(calls === "vsv", "array evaluation order");
  const a: [number, number] = [1, 2];
  const copied: [number, number, number] = [...a, change(a)];
  assert(copied[0] === 1 && copied[2] === 4 && a[0] === 9, "spread snapshot");
  const ordinary = [...a, change(a)];
  assert(ordinary[0] === 9 && ordinary[2] === 4, "array snapshot");
}
