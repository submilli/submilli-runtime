function boom(n: number): never { throw new Error("boom"); }
function call(f: (n: number) => void): void { f(1); }
function pick<U>(f: (n: number) => U): (n: number) => U { return f; }
function main(): void {
  let caught = 0;
  const arrow = (n: number) => boom(n);
  const sameA: (n: number) => void = arrow;
  const sameB: (n: number) => void = arrow;
  assert(sameA === sameB, "coercion preserves identity");
  const different: (n: number) => void = (n: number): never => boom(n);
  assert(sameA !== different, "distinct closures remain distinct");
  let nullable: ((n: number) => void) | null = arrow;
  try { nullable?.(1); } catch (e) { caught++; }
  const assigned: (n: number) => void = boom;
  try { call(boom); } catch (e) { caught++; }
  try { call(arrow); } catch (e) { caught++; }
  try { assigned(1); } catch (e) { caught++; }
  try { call(pick((n: number) => boom(n))); } catch (e) { caught++; }
  assert(caught === 5, "all callbacks throw catchable errors");
}
