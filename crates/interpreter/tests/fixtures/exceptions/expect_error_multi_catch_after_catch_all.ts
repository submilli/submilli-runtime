// expect-error: unreachable `catch` clause: `HttpError` extends `Error`
// expect-error: duplicate `catch` clause for `Error`

class HttpError extends Error {
  constructor(m: string) {
    super(m);
    this.name = "HttpError";
  }
}

function main(): void {
  try {
    throw new HttpError("x");
  } catch (e) {
    assert(true, "catch-all");
  } catch (e: HttpError) {
    assert(false, "unreachable after the catch-all");
  }

  try {
    throw new Error("y");
  } catch (e: Error) {
    assert(true, "explicit Error arm");
  } catch (f) {
    assert(false, "untyped duplicate of the Error arm");
  }
}
