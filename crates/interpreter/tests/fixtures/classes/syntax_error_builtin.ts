class ConfigParseError extends SyntaxError {
  constructor(message: string) {
    super(message);
    this.name = "ConfigParseError";
  }
}

function main(): void {
  // Constructible, with the right name and message.
  const e = new SyntaxError("bad input");
  assert(e.message === "bad input", "message field");
  assert(e.name === "SyntaxError", "name field");
  assert(e instanceof SyntaxError, "instanceof own class");
  assert(e instanceof Error, "instanceof parent");
  assert(Error.isError(e), "Error.isError sees the subclass");
  assert(e.toString() === "SyntaxError: bad input", "toString");

  // Sibling built-in subclasses are distinct (checked through the shared
  // Error type — direct sibling instanceof is a static always-false error).
  const range: Error = new RangeError("out of range");
  assert(!(range instanceof SyntaxError), "RangeError is not SyntaxError");
  const asError: Error = e;
  assert(!(asError instanceof TypeError), "SyntaxError is not TypeError");
  const base = new Error("plain");
  assert(!(base instanceof SyntaxError), "base Error is not SyntaxError");

  // Typed catch filters: SyntaxError arm binds a thrown SyntaxError.
  let caught = "";
  try {
    throw new SyntaxError("thrown");
  } catch (e: SyntaxError) {
    caught = e.name + ":" + e.message;
  }
  assert(caught === "SyntaxError:thrown", "typed catch binds SyntaxError");

  // A base Error skips the SyntaxError arm and lands in the catch-all.
  let arm = "";
  try {
    throw new Error("base");
  } catch (e: SyntaxError) {
    arm = "syntax";
  } catch (e) {
    arm = "base:" + e.name;
  }
  assert(arm === "base:Error", "base Error skips the SyntaxError arm");

  // Runtime parse failures throw SyntaxError: BigInt of an invalid literal.
  let bigintErr = "";
  try {
    const b = BigInt("not-a-number");
    bigintErr = b.toString();
  } catch (e: SyntaxError) {
    bigintErr = e.name;
  }
  assert(bigintErr === "SyntaxError", "BigInt of invalid literal is SyntaxError");

  // JSON.parse of malformed JSON.
  let jsonErr = "";
  try {
    const n = JSON.parse("{oops") as number;
    jsonErr = n.toString();
  } catch (e: SyntaxError) {
    jsonErr = e.name;
  }
  assert(jsonErr === "SyntaxError", "malformed JSON.parse is SyntaxError");

  // A type mismatch on well-formed JSON is a TypeError, not SyntaxError.
  let shapeArm = "";
  try {
    const n = JSON.parse("\"well-formed\"") as number;
    shapeArm = n.toString();
  } catch (e: SyntaxError) {
    shapeArm = "syntax";
  } catch (e) {
    shapeArm = "other:" + e.name;
  }
  assert(shapeArm === "other:TypeError", "JSON type mismatch is TypeError");

  // new RegExp of an invalid pattern, and of invalid flags.
  let regexErr = "";
  try {
    const re: RegExp = new RegExp("(", "");
    regexErr = "compiled";
  } catch (e: SyntaxError) {
    regexErr = e.name;
  }
  assert(regexErr === "SyntaxError", "invalid RegExp pattern is SyntaxError");

  let flagErr = "";
  try {
    const re: RegExp = new RegExp("abc", "q");
    flagErr = "compiled";
  } catch (e: SyntaxError) {
    flagErr = e.name;
  }
  assert(flagErr === "SyntaxError", "invalid RegExp flag is SyntaxError");

  // Uint8Array.fromBase64 of malformed input.
  let base64Err = "";
  try {
    const bytes = Uint8Array.fromBase64("!!!!");
    base64Err = bytes.length.toString();
  } catch (e: SyntaxError) {
    base64Err = e.name;
  }
  assert(base64Err === "SyntaxError", "malformed fromBase64 is SyntaxError");

  // User subclasses of SyntaxError chain through it to Error.
  const custom = new ConfigParseError("unterminated section");
  assert(custom instanceof ConfigParseError, "instanceof own class");
  assert(custom instanceof SyntaxError, "instanceof SyntaxError parent");
  assert(custom instanceof Error, "instanceof Error root");
  assert(custom.name === "ConfigParseError", "subclass name");
}
