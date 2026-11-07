function main(): void {
  let conditional: number;
  conditional = 1;
  true && (conditional = 2);
  assert(conditional === 2, "conditional writes preserve prior assignment");

  let captured: number;
  captured = 1;
  const change = (): void => { captured = 2; };
  assert(captured === 1, "capture preserves prior assignment");
  change();
  assert(captured === 2, "captured writes cannot unassign a local");
  captured++;
  captured += 2;
  assert(captured === 5, "updates after invalidation read the assigned type");

  let caught: number;
  caught = 1;
  try { caught = 2; throw new Error("test"); } catch {
    assert(caught === 2, "catch keeps assignment from before try");
  }
  let final: number;
  final = 3;
  try { final = 4; } finally {
    assert(final === 4, "finally keeps assignment from before try");
  }
  let action: () => number;
  action = (): number => 7;
  const replace = (): void => { action = (): number => 8; };
  assert(action() === 7, "call assigned function before mutation");
  replace();
  assert(action() === 8, "call assigned function after mutation");
}
