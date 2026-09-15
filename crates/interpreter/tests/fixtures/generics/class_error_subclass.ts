// A generic subclass of the host-implemented `Error`: inherited `message`, a
// `T` payload, and a method mixing both, thrown from and caught in a generic
// context.
class Wrapped<T> extends Error {
  payload: T;

  constructor(payload: T, message: string) {
    super(message);
    this.payload = payload;
  }

  describe(): string {
    return this.message + ":" + JSON.stringify(this.payload);
  }
}

function boom<T>(payload: T): void {
  throw new Wrapped(payload, "bad");
}

function main(): void {
  let seen = 0;
  try {
    boom("s");
  } catch (e) {
    assert(e instanceof Wrapped, "generic Error subclass narrows via instanceof");
    if (e instanceof Wrapped) {
      assert(e.message === "bad", "inherited Error field");
      assert(e.describe() === "bad:\"s\"", "method mixing this.message and T");
      seen = seen + 1;
    }
  }
  try {
    boom(7);
  } catch (e: Wrapped) {
    assert(e.message === "bad", "typed catch on the generic subclass");
    const p: unknown = e.payload;
    assert(typeof p === "number" && p === 7, "payload at a second instantiation");
    seen = seen + 1;
  }
  assert(seen === 2, "both clauses ran");
}
