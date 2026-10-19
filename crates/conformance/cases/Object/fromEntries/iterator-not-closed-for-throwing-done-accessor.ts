// test262: test/built-ins/Object/fromEntries/iterator-not-closed-for-throwing-done-accessor.js
// Source: https://github.com/tc39/test262/blob/d53ebc8b1931d9f1541e43d59650776e4b766248/test/built-ins/Object/fromEntries/iterator-not-closed-for-throwing-done-accessor.js
// iterator/close replace Symbol.iterator/return. A class supplies the throwing
// getter; done is typed false and value supplies the required, unreachable yield
// field. DummyError extends Error; a typed catch preserves assert.throws type checking.
// Error represents the harness Test262Error.

class DummyError extends Error {
  constructor() { super("dummy"); }
}
class ThrowingDone {
  value: [string, number] = ["key", 1];
  get done(): false { throw new DummyError(); }
}
function main(): void {
  const iterable: Iterable<[string, number]> = {
    iterator: (): Iterator<[string, number]> => ({
      next: (): IteratorResult<[string, number]> => { return new ThrowingDone(); },
      close: (): void => { throw new Error("should not call return"); },
    }),
  };
  let caught = false;
  try { Object.fromEntries(iterable); } catch (e: DummyError) { caught = true; }
  assert(caught, "done getter throws DummyError");
}
