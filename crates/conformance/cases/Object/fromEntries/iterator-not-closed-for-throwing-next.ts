// test262: test/built-ins/Object/fromEntries/iterator-not-closed-for-throwing-next.js
// Adaptation: iterator()/close() represent Symbol.iterator/return; DummyError is an Error subclass.
class DummyError extends Error {
  constructor() { super("dummy"); }
}
function main(): void {
  const iterable: Iterable<[string, number]> = {
    iterator: (): Iterator<[string, number]> => ({
      next: (): IteratorResult<[string, number]> => { throw new DummyError(); },
      close: (): void => { throw new Error("should not call return"); },
    }),
  };
  let caught = false;
  try { Object.fromEntries(iterable); } catch (e: DummyError) { caught = true; }
  assert(caught, "next throws DummyError");
}
