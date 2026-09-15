// expect-error: duplicate `catch` clause for `ParseError`

class ParseError extends Error {
  constructor(m: string) {
    super(m);
    this.name = "ParseError";
  }
}

function main(): void {
  try {
    throw new ParseError("x");
  } catch (e: ParseError) {
    assert(true, "first arm");
  } catch (e: ParseError) {
    assert(false, "never reached");
  }
}
