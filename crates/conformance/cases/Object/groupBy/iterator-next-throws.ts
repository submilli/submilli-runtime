// test262: test/built-ins/Object/groupBy/iterator-next-throws.js
// Adaptation: iterator() represents Symbol.iterator; a class replaces this-returning object methods.
class Test262Error extends Error { constructor(message: string) { super(message); } }
class ThrowingIterator {
  iterator(): Iterator<number> { return this; }
  next(): IteratorResult<number> { throw new Test262Error("next() method was called"); }
}
function main(): void {
  let caught = false;
  try { Object.groupBy<number, number>(new ThrowingIterator(), (): number => 1); }
  catch (e: Test262Error) { caught = true; }
  assert(caught, "iterator throws Test262Error");
}
