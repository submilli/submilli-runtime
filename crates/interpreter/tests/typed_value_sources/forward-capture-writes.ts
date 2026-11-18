function main(): number {
  let effects = 0;
  let caught = 0;
  const effectCount = (): number => effects;
  const value = (): number => { effects++; return 9; };
  const write = (): void => { late = value(); };
  const bump = (): number => late++;
  try { write(); } catch (e: unknown) { if (e instanceof ReferenceError) caught++; }
  try { bump(); } catch (e: unknown) { if (e instanceof ReferenceError) caught++; }
  let late = 2;
  if (caught !== 2 || effectCount() !== 1) return 1;
  if (bump() !== 2) return 2;
  write();
  if (late !== 9 || effectCount() !== 2) return 3;
  return 0;
}
