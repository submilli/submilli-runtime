// test262: test/built-ins/Object/fromEntries/iterator-closed-for-throwing-entry-key-tostring.js
// Adaptation: class key and a tuple mutated through an unknown[] alias replace the indexed entry object.
// iterator()/close() represent Symbol.iterator/return; DummyError is an Error subclass.
class DummyError extends Error {
  constructor() { super("dummy"); }
}
class Key {
  toString(): string { throw new DummyError(); }
}
function main(): void {
  let returned = false;
  const iterable: Iterable<[string, number]> = {
    iterator: (): Iterator<[string, number]> => {
      let advanced = false;
      return {
        next: (): IteratorResult<[string, number]> => {
          if (advanced) throw new Error("should only advance once");
          advanced = true;
          const pair: [string, number] = ["key", 0];
          const erased: unknown[] = pair;
          erased[0] = new Key();
          return { done: false, value: pair };
        },
        close: (): void => {
          if (returned) throw new Error("should only return once");
          returned = true;
        },
      };
    },
  };
  let caught = false;
  try { Object.fromEntries(iterable); } catch (e: DummyError) { caught = true; }
  assert(caught, "key toString throws DummyError");
  assert(returned, "iterator should be closed when key toString throws");
}
