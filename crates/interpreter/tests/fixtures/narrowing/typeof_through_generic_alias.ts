// A `typeof` guard on an operand whose type is a *generic alias* must run at
// runtime, not fold to a constant. The operand's erasure is what makes the
// answer unknowable at compile time, and an alias of a type parameter is erased
// exactly as the bare parameter is — reading the alias as a concrete type folds
// the guard to `false` with no diagnostic, so the wrong branch is taken.
type Box<T> = T;
type MaybeBox<T> = Box<T> | null;
// An alias of an alias of the parameter is erased just as deeply.
type Nested<T> = Box<T>;

function tag<T>(x: Box<T>): string {
  if (typeof x === "object") {
    return "object";
  }
  if (typeof x === "function") {
    return "function";
  }
  if (typeof x === "number") {
    return "number";
  }
  return "other";
}

function tagNullable<T>(x: MaybeBox<T>): string {
  if (typeof x === "object") {
    return "object";
  }
  return "other";
}

// A composite whose element type is erased still has a fixed runtime
// classification — the array is an object whatever `T` turns out to be.
function tagArray<T>(xs: Box<T>[]): string {
  return typeof xs === "object" ? "object" : "other";
}

function tagNested<T>(x: Nested<T>): string {
  if (typeof x === "string") {
    return "string";
  }
  if (typeof x === "boolean") {
    return "boolean";
  }
  return "other";
}

function notNumber<T>(x: Box<T>): boolean {
  return typeof x !== "number";
}

class Tagger<T> {
  tag(x: Box<T>): string {
    return typeof x === "number" ? "number" : "other";
  }
}

function main(): void {
  assert(tag<number[]>([1]) === "object", "array argument tags as object");
  assert(tag<number>(1) === "number", "number argument tags as number");
  assert(tag<string>("s") === "other", "string argument falls through");
  assert(
    tag<(n: number) => number>((n: number): number => n) === "function",
    "closure argument tags as function",
  );
  // `null` is an object under JS `typeof`, and so is a non-null member.
  assert(tagNullable<number[]>(null) === "object", "null tags as object");
  assert(tagNullable<number>(2) === "other", "number member falls through");
  assert(tagArray<number>([1]) === "object", "aliased element type is erased");

  assert(tagNested<string>("s") === "string", "alias of an alias, string tag");
  assert(tagNested<boolean>(true) === "boolean", "alias of an alias, boolean tag");
  assert(tagNested<number>(1) === "other", "alias of an alias falls through");

  assert(notNumber<string>("s"), "negated guard on an erased operand");
  assert(!notNumber<number>(1), "negated guard, matching operand");

  assert(new Tagger<number>().tag(1) === "number", "a class type parameter is erased too");
  assert(new Tagger<string>().tag("x") === "other", "second instantiation");
}
