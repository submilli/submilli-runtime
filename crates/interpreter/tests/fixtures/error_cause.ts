// SUB-1817: error constructors take `ErrorOptions`, whose `cause` the error
// keeps, and an `undefined` message takes the default, as in JavaScript.
class AppError extends Error {
  code: number;
  constructor(message: string, code: number, cause?: unknown) {
    super(message, { cause });
    this.name = "AppError";
    this.code = code;
  }
}
class GetterOptions {
  get cause(): unknown {
    return "from getter";
  }
}
class Quiet extends Error {
  constructor(message?: string) {
    super(message);
  }
}
function main(): void {
  const root = new TypeError("bad input");
  const wrapped = new Error("wrapped", { cause: root });
  assert(wrapped.cause === root && wrapped.cause instanceof TypeError);
  assert(new Error("plain").cause === undefined);
  assert(new Error("empty options", {}).cause === undefined);
  assert(Object.keys(wrapped).length === 0);
  const range = new RangeError("r", { cause: "text" });
  assert(range.cause === "text" && String(range) === "RangeError: r");
  const app = new AppError("app", 7, wrapped);
  assert(app.code === 7 && app.cause === wrapped && app.message === "app");
  assert(new AppError("no cause", 1).cause === undefined);
  try {
    throw new SyntaxError("s", { cause: 42 });
  } catch (error) {
    assert(error instanceof Error && error.cause === 42);
  }
  const missing: string | undefined = undefined;
  assert(new Error(missing).message === "" && new URIError(undefined, {}).message === "");
  assert(new Quiet().message === "" && new Quiet("q").message === "q");
  assert(new Error("g", new GetterOptions()).cause === "from getter");
}
