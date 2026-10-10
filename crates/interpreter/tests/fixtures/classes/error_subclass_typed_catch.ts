// `catch (e: MyError)` — a subclass-typed catch binds only that error type
// (by nominal identity, so a same-shape sibling never mis-binds) and re-raises
// everything else to the next handler.
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

class HttpError extends Error {
  status: number;
  constructor(message: string, status: number) {
    super(message);
    this.name = "HttpError";
    this.status = status;
  }
}

class Timeout extends HttpError {
  constructor() {
    super("timed out", 408);
    this.name = "Timeout";
  }
}

function main(): void {
  // Match: binds and narrows without an instanceof dance.
  let status = 0;
  try {
    throw new HttpError("not found", 404);
  } catch (e: HttpError) {
    status = e.status;
  }
  assert(status === 404, "typed catch binds the subclass directly");

  // Mismatch re-raises to the outer handler — including the same-shape
  // sibling, which shape-based filtering would have mis-bound.
  let trail = "";
  try {
    try {
      throw new NotFoundError("missing");
    } catch (e: ConflictError) {
      trail = trail + "conflict!";
      assert(false, "sibling class must not bind");
    }
  } catch (e) {
    trail = trail + "outer:" + e.name;
  }
  assert(trail === "outer:NotFoundError", "mismatch re-raises the original error");

  // A subclass instance matches a typed catch of an ancestor class.
  let seen = "";
  try {
    throw new Timeout();
  } catch (e: HttpError) {
    seen = e.name + ":" + e.status.toString();
  }
  assert(seen === "Timeout:408", "typed catch accepts subclasses of the annotation");
}
