import { ParseError, failParse, failIo } from "@test/errs";

function main(): void {
  // Match against the imported subclass — binds and exposes its fields.
  let line = 0;
  try {
    failParse();
  } catch (e: ParseError) {
    line = e.line;
  }
  assert(line === 3, "typed catch binds the imported subclass");

  // A different imported subclass re-raises past the typed clause.
  let trail = "";
  try {
    try {
      failIo();
    } catch (e: ParseError) {
      assert(false, "IoError must not bind a ParseError clause");
    }
  } catch (e) {
    trail = e.name;
  }
  assert(trail === "IoError", "mismatch re-raises across the package boundary");
}
