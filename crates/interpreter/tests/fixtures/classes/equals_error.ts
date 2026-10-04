// Errors and subclasses compare by reference identity, including erased values.
class NotFoundError extends Error {
  constructor(m: string) {
    super(m);
    this.name = "NotFoundError";
  }
}

function main(): void {
  assert(new Error("x") !== new Error("x"));
  assert(new Error("x") !== new Error("y"));

  const base: Error = new Error("x");
  const sub: Error = new NotFoundError("x");
  assert(base === base);
  assert(sub === sub);
  const keys = new Map<Error, number>();
  keys.set(sub, 7);
  sub.message = "changed";
  assert(keys.get(sub) === 7);
  assert(keys.get(new NotFoundError("changed")) === null);
  assert(base !== sub);
  assert(sub !== base);

  assert(new NotFoundError("x") !== new NotFoundError("x"));
  assert(new NotFoundError("x") !== new NotFoundError("y"));
}
