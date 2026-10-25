// `+` concatenates an `Error` onto a string as `String(error)` converts it:
// `"<name>: <message>"`, as in JavaScript, through the error's own `toString`.
class NotFound extends Error {}
class Named extends Error {
  name: string = "Named";
}
class Custom extends Error {
  toString(): string {
    return "custom!";
  }
}
function main(): void {
  assert("err " + new Error("x") === "err Error: x");
  assert("err " + new TypeError("x") === "err TypeError: x");
  assert(new RangeError("r") + "!" === "RangeError: r!");
  assert("err " + new Error("") === "err Error");
  assert("err " + new Named("n") === "err Named: n");
  assert("err " + new Custom("c") === "err custom!");
  try {
    throw new NotFound("missing");
  } catch (e) {
    assert("caught " + e === "caught Error: missing");
    let log = "log: ";
    log += e;
    assert(log === "log: Error: missing");
    assert("err " + e === `err ${e}`);
  }
}
