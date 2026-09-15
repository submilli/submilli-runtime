// Nominal identity: same-shape sibling classes (same parent, same member
// layout) canonicalize to the same WasmGC type, so `instanceof` must not rely
// on shape — a NotFoundError is not a ConflictError even though their types
// structurally unify.
class NotFoundError extends Error {
  constructor(m: string) {
    super(m);
    this.name = "NotFoundError";
  }
}

class ConflictError extends Error {
  constructor(m: string) {
    super(m);
    this.name = "ConflictError";
  }
}

class Shape {}
class Circle extends Shape {}
class Square extends Shape {}

function main(): void {
  const e: Error = new NotFoundError("missing");
  assert(e instanceof NotFoundError);
  assert(!(e instanceof ConflictError));
  assert(e instanceof Error);

  const s: Shape = new Circle();
  assert(s instanceof Circle);
  assert(!(s instanceof Square));
  assert(s instanceof Shape);
}
