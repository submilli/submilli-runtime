function laterDefault(read: () => number = () => later, value: number = read(), later: number = 42): number {
  return value;
}
function laterRequired(read: () => number = () => later, value: number = read(), later: number): number {
  return value;
}
function laterPattern(read: () => number = () => later, value: number = read(), { later = 42 }: { later?: number } = {}): number {
  return value;
}
function main(): void {
  let defaultCaught = false;
  try { laterDefault(); } catch (error: Error) { defaultCaught = true; }
  assert(defaultCaught, "invoking a closure before later default initializes must throw");
  let requiredCaught = false;
  try { laterRequired(undefined, undefined, 42); } catch (error: Error) { requiredCaught = true; }
  assert(requiredCaught, "a supplied later parameter remains uninitialized until its turn");
  let patternCaught = false;
  try { laterPattern(); } catch (error: Error) { patternCaught = true; }
  assert(patternCaught, "invoking a closure before later pattern binding initializes must throw");
}
