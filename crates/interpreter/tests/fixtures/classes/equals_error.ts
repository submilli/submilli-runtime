// Error follows class `===` semantics: exact-Error guard + structural
// message/name compare (host-side twin of the per-class equals bodies).
class NotFoundError extends Error {
  constructor(m: string) {
    super(m);
    this.name = "NotFoundError";
  }
}

function main(): void {
  assert(new Error("x") === new Error("x"));
  assert(new Error("x") !== new Error("y"));

  const base: Error = new Error("x");
  const sub: Error = new NotFoundError("x");
  assert(base !== sub);
  assert(sub !== base);

  assert(new NotFoundError("x") === new NotFoundError("x"));
  assert(new NotFoundError("x") !== new NotFoundError("y"));
}
