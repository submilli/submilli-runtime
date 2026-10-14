let writes = "";
class PartialTarget {
  set a(value: number) { writes += "a"; }
  set b(value: number) { writes += "b"; throw new Error("setter failed"); }
  set c(value: number) { writes += "c"; }
}
class ThrowingValue {
  done: false = false;
  get value(): number { throw new Error("value failed"); }
}
function main(): void {
  let message = "";
  try { Object.assign(new PartialTarget(), { a: 1, b: 2, c: 3 }); }
  catch (e: Error) { message = e.message; }
  assert(message === "setter failed" && writes === "ab");
  let closed = false;
  const values: Iterable<number> = {
    iterator: (): Iterator<number> => ({
      next: (): IteratorResult<number> => new ThrowingValue(),
      close: (): void => { closed = true; },
    }),
  };
  message = "";
  try { Object.groupBy(values, (): string => "key"); }
  catch (e: Error) { message = e.message; }
  assert(message === "value failed" && !closed);
  const source: Iterable<number> = {
    iterator: (): Iterator<number> => ({
      next: (): IteratorResult<number> => ({ done: false, value: 1 }),
      close: (): void => { closed = true; throw new Error("close failed"); },
    }),
  };
  message = "";
  try { Object.groupBy(source, (): string => { throw new Error("callback failed"); }); }
  catch (e: Error) { message = e.message; }
  assert(closed && message === "callback failed");
  const healthy = Object.groupBy([1], (): string => "key");
  assert(healthy.key![0] === 1);
}
