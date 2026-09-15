// expect-error: cannot be tested at a specific instantiation
// A `catch` clause filters on the nominal brand, which carries no type
// arguments — the bare `catch (e: MyErr)` form is the sound spelling.
class MyErr<T> extends Error {
  payload: T;
  constructor(message: string, payload: T) {
    super(message);
    this.payload = payload;
  }
}

function main(): void {
  try {
    throw new MyErr<string>("boom", "str");
  } catch (e: MyErr<number>) {
    assert(e.payload + 1 === 2);
  }
}
