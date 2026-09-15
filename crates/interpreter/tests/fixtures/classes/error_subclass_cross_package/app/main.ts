import { ParseError, parseOrThrow } from "@test/errs";

function main(): void {
  assert(parseOrThrow("ok") === "ok", "non-throwing path");
  try {
    parseOrThrow("");
    assert(false, "should have thrown");
  } catch (e) {
    assert(e.name === "ParseError", "cross-package subclass name survives the throw");
    assert(e.message === "empty input", "message survives");
    if (e instanceof ParseError) {
      assert(e.line === 1, "narrowed cross-package subclass exposes its fields");
    } else {
      assert(false, "instanceof narrows against the imported subclass");
    }
  }
  const direct = new ParseError("direct", 7);
  assert(direct instanceof Error, "imported subclass is an Error");
  assert(direct.line === 7, "constructed directly across the boundary");
}
