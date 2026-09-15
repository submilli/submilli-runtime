// Thrown subclass instances are caught by untyped `catch` as `Error`, keep
// their fields, and narrow back via `instanceof`.
class HttpError extends Error {
  status: number;
  constructor(message: string, status: number) {
    super(message);
    this.name = "HttpError";
    this.status = status;
  }
}

class Empty extends Error {}

function main(): void {
  let caught = "";
  try {
    throw new HttpError("not found", 404);
  } catch (e) {
    caught = e.name + ": " + e.message;
    if (e instanceof HttpError) {
      assert(e.status === 404, "narrowed subclass exposes its own fields");
    } else {
      assert(false, "caught value narrows to the thrown subclass");
    }
  }
  assert(caught === "HttpError: not found", "catch binding reads subclass fields as Error");

  // Implicit-ctor subclass inherits the parent constructor params.
  try {
    throw new Empty("bare");
  } catch (e) {
    assert(e.message === "bare", "implicit ctor forwards to super");
    assert(e.name === "Error", "implicit ctor keeps the default name");
  }
}
