// A narrowed `T` passed to `V | undefined` binds `V` to the whole narrowing,
// as tsc infers it, so the result may still be `null` and doesn't fit `{}`.
// expect-error: expected `{}`, got `T & ({} | null)`
// expect-error-count: 1
function required<V>(value: V | undefined): V {
  if (value === undefined) {
    throw new Error("missing");
  }
  return value;
}

function present<T>(value: T): {} {
  if (value !== undefined) {
    return required(value);
  }
  return 0;
}

function main(): void {
  console.log(present<number | null>(null));
}
