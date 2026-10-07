// Assigning a literal the declared union doesn't name narrows the binding to
// the member holding it, as TypeScript does, even when the literal came from a
// narrowing: after `x = k` under `k === 2`, `x` is a number, not `2`.
let moduleValue: string | number = "a";

function pick(k: number): string | number {
  let x: string | number = "a";
  if (k === 2) {
    x = k;
  }
  if (x === 3) {
    return "three";
  }
  return x;
}

function pickInLoop(): string | number {
  let y: string | number = "a";
  for (const j of [1, 2]) {
    if (j === 2) {
      y = j;
    }
  }
  if (y === 3) {
    return "three";
  }
  return y;
}

function pickModule(k: number): string | number {
  if (k === 2) {
    moduleValue = k;
  }
  if (moduleValue === 3) {
    return "three";
  }
  return moduleValue;
}

function main(): void {
  assert(pick(2) === 2);
  assert(pick(1) === "a");
  assert(pickInLoop() === 2);
  assert(pickModule(2) === 2);
}
