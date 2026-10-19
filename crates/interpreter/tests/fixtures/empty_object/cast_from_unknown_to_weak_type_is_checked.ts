// A `{}` value cast to a weak object type converts as an annotated one does,
// but a cast from `unknown` stays a checked cast, as every `as` from
// `unknown` is: a number is not an object of the target's shape.
interface Options { a?: number }

function fromUnknown(value: unknown): string {
  try {
    return String((value as Options).a);
  } catch (e) {
    return e instanceof TypeError ? "TypeError" : "other";
  }
}

function fromEmptyObject(value: {}): string {
  return String((value as Options).a);
}

function main(): void {
  assert(fromUnknown(42) === "TypeError", "a cast from unknown checks the value");
  assert(fromUnknown({ a: 1 }) === "1", "and passes an object of the shape");
  assert(fromEmptyObject(42) === "undefined", "a cast from {} converts");
}
