// Every value answers to exactly one `typeof` tag. A type constructor with no
// arm in the fact table answered `false` to all five, which is unsatisfiable —
// the guard compiled, took the wrong branch, and said nothing.

class C {
  v: number = 1;
}

enum E {
  A = 1,
  B = 2,
}

enum SE {
  A = "a",
  B = "b",
}

function tagOf<T>(x: T): string {
  if (typeof x === "string") return "string";
  if (typeof x === "number") return "number";
  if (typeof x === "boolean") return "boolean";
  if (typeof x === "function") return "function";
  if (typeof x === "object") return "object";
  return "none";
}

// Where the operand is a union the fold can't decide, the guard must narrow to
// the right members on *both* sides — a guard that answers `true` but narrows to
// `never` is the worse half of the bug.
function objOrNumber(x: C | number): string {
  if (typeof x === "object") {
    return "obj:" + x.v.toString();
  }
  return "num:" + x.toString();
}

function u8OrString(x: Uint8Array | string): string {
  if (typeof x === "object") {
    return "u8:" + x.length.toString();
  }
  return "str:" + x;
}

function enumOrString(x: E | string): string {
  if (typeof x === "number") {
    return "enum:" + x.toString();
  }
  return "str:" + x;
}

function strEnumOrNumber(x: SE | number): string {
  if (typeof x === "string") {
    return x === SE.A ? "senum:a" : "senum:?";
  }
  return "num:" + x.toString();
}

// `unknown` is the language's dynamic type, so it is where the tag question
// gets asked most — and it is the one operand type that decides nothing
// statically. Every tag has to run at run time.
function tagOfUnknown(x: unknown): string {
  if (typeof x === "string") {
    return "string";
  }
  if (typeof x === "number") {
    return "number";
  }
  if (typeof x === "boolean") {
    return "boolean";
  }
  if (typeof x === "function") {
    return "function";
  }
  if (typeof x === "object") {
    return "object";
  }
  return "none";
}

function main(): void {
  // Statically folded: the operand's type decides the answer.
  const c = new C();
  assert(typeof c === "object", "a class instance is an object");
  assert(typeof new Error("x") === "object", "an Error is an object");
  assert(typeof Uint8Array.alloc(1) === "object", "a Uint8Array is an object");
  assert(typeof new Map<string, number>() === "object", "a Map is an object");
  assert(typeof new Set<string>() === "object", "a Set is an object");
  assert(typeof /ab/ === "object", "a regex is an object");
  assert(typeof [1, 2] === "object", "an array is an object");
  const t: [number, string] = [1, "a"];
  assert(typeof t === "object", "a tuple is an object");
  assert(typeof E.A === "number", "a numeric enum member is a number");
  assert(typeof SE.A === "string", "a string enum member is a string");

  // The other four tags stay false for each of them.
  assert(!(typeof c === "function"), "a class instance is not a function");
  assert(!(typeof E.A === "string"), "a numeric enum member is not a string");
  assert(!(typeof SE.A === "number"), "a string enum member is not a number");

  // Not folded: an erased operand runs the classification at runtime, which
  // must agree with the fold.
  assert(tagOf(c) === "object", "runtime: class instance");
  assert(tagOf(new Error("x")) === "object", "runtime: Error");
  assert(tagOf(Uint8Array.alloc(1)) === "object", "runtime: Uint8Array");
  assert(tagOf(new Map<string, number>()) === "object", "runtime: Map");
  assert(tagOf(new Set<string>()) === "object", "runtime: Set");
  assert(tagOf(/ab/) === "object", "runtime: regex");
  assert(tagOf([1, 2]) === "object", "runtime: array");
  assert(tagOf(E.A) === "number", "runtime: numeric enum member");
  assert(tagOf(SE.A) === "string", "runtime: string enum member");
  assert(tagOf("s") === "string", "runtime: string");
  assert(tagOf(1) === "number", "runtime: number");
  assert(tagOf(true) === "boolean", "runtime: boolean");
  assert(tagOf(main) === "function", "runtime: function");
  assert(tagOf(null) === "object", "runtime: null is an object, as in JS");
  // `bigint` is not one of the five tags the language accepts, so a bigint
  // answers none of them — including "object", which it is not.
  assert(tagOf(1n) === "none", "runtime: bigint answers no supported tag");

  assert(objOrNumber(new C()) === "obj:1", "class narrowed in the true branch");
  assert(objOrNumber(3) === "num:3", "number narrowed in the false branch");
  assert(u8OrString(Uint8Array.alloc(4)) === "u8:4", "Uint8Array true branch");
  assert(u8OrString("hi") === "str:hi", "string false branch");
  assert(enumOrString(E.B) === "enum:2", "numeric enum true branch");
  assert(enumOrString("z") === "str:z", "string false branch");
  assert(strEnumOrNumber(SE.A) === "senum:a", "string enum true branch");
  assert(strEnumOrNumber(9) === "num:9", "number false branch");

  assert(tagOfUnknown("a") === "string", "unknown: string");
  assert(tagOfUnknown(1) === "number", "unknown: number");
  assert(tagOfUnknown(true) === "boolean", "unknown: boolean");
  assert(tagOfUnknown(main) === "function", "unknown: function");
  assert(tagOfUnknown(c) === "object", "unknown: class instance");
  assert(tagOfUnknown([1]) === "object", "unknown: array");
  assert(tagOfUnknown(new Map<string, number>()) === "object", "unknown: Map");
  assert(tagOfUnknown(Uint8Array.alloc(1)) === "object", "unknown: Uint8Array");
  assert(tagOfUnknown(null) === "object", "unknown: null");
  assert(tagOfUnknown(1n) === "none", "unknown: bigint answers no tag");
  assert(tagOfUnknown(E.A) === "number", "unknown: numeric enum member");
  const u: unknown = "hi";
  if (typeof u === "string") {
    assert(u.length === 2, "a primitive tag still narrows `unknown`");
  } else {
    assert(false, "`unknown` should have narrowed to string");
  }

  // Host-backed values are `$Object` subtypes the guest holds opaquely; they
  // reach the runtime test through no positive list, so only the complement
  // form classifies them.
  assert(tagOf(Temporal.Instant.fromEpochMilliseconds(0)) === "object", "runtime: Temporal");
}
