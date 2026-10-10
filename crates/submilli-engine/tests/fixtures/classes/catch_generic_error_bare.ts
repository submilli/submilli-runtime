// A generic error subclass is caught by its bare name; the payload binds at
// erased args, so reading it needs a narrowing check rather than a claim the
// runtime never verified.
class MyErr<T> extends Error {
  payload: T;
  constructor(message: string, payload: T) {
    super(message);
    this.payload = payload;
  }
}

function thrower(): void {
  throw new MyErr<string>("boom", "str");
}

function main(): void {
  let caught = false;
  try {
    thrower();
  } catch (e: MyErr) {
    caught = true;
    assert(e.message === "boom", "generic error subclass caught by bare name");
    const p: unknown = e.payload;
    assert(typeof p === "string" && p === "str", "payload narrows from unknown");
  }
  assert(caught, "the clause ran");
}
