class Key {
  toString(): string { throw new Error("entry failed"); }
}
function main(): void {
  const pair: [string, number] = ["key", 1];
  const erased: unknown[] = pair;
  erased[0] = new Key();
  let closed = false;
  const iterable: Iterable<[string, number]> = {
    iterator: (): Iterator<[string, number]> => ({
      next: (): IteratorResult<[string, number]> => ({ done: false, value: pair }),
      close: (): void => { closed = true; throw new Error("close failed"); },
    }),
  };
  let message = "";
  try { Object.fromEntries(iterable); } catch (e: Error) { message = e.message; }
  assert(message === "entry failed");
  assert(closed);
}
