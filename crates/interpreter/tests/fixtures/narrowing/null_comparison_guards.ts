function reversed(x: number | null): number {
  if (null === x) { return 0; }
  return x + 1;
}
function main(): void {
  moreGuards();
  const e: number = 1;
  assert(e !== null || true, "non-null OR");
  assert(!(e === null && true), "non-null AND");
  let i: bigint = 0n;
  if (i !== null) {}
  assert(i + 1n === 1n, "guard must not widen");
  assert(reversed(null) === 0);
  assert(reversed(2) === 3);
}

let calls: number = 0;
function effect(): number { calls += 1; return 1; }
function unknownNull(x: unknown): boolean {
  if (null !== x) { return false; }
  return true;
}
function moreGuards(): void {
  assert(effect() !== null || false);
  assert(!(null === effect() && true));
  assert((effect() === null ? false : true));
  assert(calls === 3, "null tests must evaluate their operands once");
  assert(unknownNull(null));
  assert(!unknownNull(1));
  let s: string = "a";
  if (null !== s) {}
  assert(s.length === 1);
  let b: boolean = true;
  if (b === null) { assert(false); }
  assert(b);
}
