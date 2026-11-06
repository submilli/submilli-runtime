// SUB-1830: an error has its own `cause` only when its options had one, as in
// JavaScript: `"cause" in e` and `Object.hasOwn(e, "cause")` are `false` when
// none was given, and `true` for a given `undefined`.
class NoCause extends Error {
  constructor(message: string) {
    super(message);
  }
}
class WithCause extends Error {
  constructor(message: string, cause: unknown) {
    super(message, { cause: cause });
  }
}
class Plain extends Error {}
class GetterOptions {
  get cause(): unknown {
    return "from getter";
  }
}
function owns(error: Error): boolean {
  const inOperator = "cause" in error;
  assert(inOperator === Object.hasOwn(error, "cause"));
  return inOperator;
}
function main(): void {
  assert(!owns(new Error("x")) && !owns(new Error("x", {})));
  assert(owns(new Error("x", { cause: undefined })) && owns(new Error("x", { cause: 1 })));
  assert(!owns(new TypeError("x")) && owns(new RangeError("x", { cause: "c" })));
  const options: ErrorOptions = {};
  const none: ErrorOptions | undefined = undefined;
  assert(!owns(new Error("x", options)) && !owns(new Error("x", none)));
  assert(owns(new Error("x", new GetterOptions())));
  assert(!owns(new NoCause("x")) && !owns(new Plain("x")));
  assert(owns(new WithCause("x", 2)) && owns(new WithCause("x", undefined)));
  const later = new Error("x");
  later.cause = undefined;
  assert(owns(later) && later.cause === undefined);
  const subLater = new NoCause("x");
  subLater.cause = 3;
  assert(owns(subLater));
  assert(Object.keys(new Error("x", { cause: 1 })).length === 0);
  assert(JSON.stringify(new Error("x", { cause: 1 })) === "{}");
  try {
    JSON.parse("{");
  } catch (error) {
    assert(error instanceof Error && !owns(error));
  }
}
