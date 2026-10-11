function deferredRest(read: () => number = () => tail.value, { head, ...tail }: { head: number; value: number } = { head: 0, value: 7 }): number {
  return read();
}
function earlyRest(read: () => number = () => tail.value, result: number = read(), { head, ...tail }: { head: number; value: number } = { head: 0, value: 7 }): number {
  return result;
}
function main(): void {
  assert(deferredRest() === 7);
  assert(deferredRest(undefined, { head: 1, value: 9 }) === 9);
  const arrow = (read: () => number = () => tail.value, { head, ...tail }: { head: number; value: number } = { head: 0, value: 8 }): number => read();
  assert(arrow() === 8);
  let threw = false;
  try { earlyRest(undefined, undefined, { head: 0, value: 9 }); } catch (error: ReferenceError) { threw = true; }
  assert(threw, "a closure cannot read a later rest binding before initialization");
}
