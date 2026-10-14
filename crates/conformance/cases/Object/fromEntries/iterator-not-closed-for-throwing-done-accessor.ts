// test262: test/built-ins/Object/fromEntries/iterator-not-closed-for-throwing-done-accessor.js
// Adaptation: iterator()/close() represent Symbol.iterator/return; DummyError is an Error subclass.
// A class getter replaces the getter on the iterator result object.
class DummyError extends Error {
  constructor() { super("dummy"); }
}
class ThrowingDone {
  value: [string, number] = ["key", 1];
  get done(): boolean { throw new DummyError(); }
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
