// A narrowed `T` is still a `T`: it fits a union that holds a narrowing of
// `T`, but not one of another type parameter `U`, as tsc reports.
// expect-error: expected `(U & ({} | undefined)) | undefined`, got `T & {}`
// expect-error: expected `U & ({} | undefined)`, got `T & number`
// expect-error-count: 2
function swap<T, U>(t: T, u: U): U | undefined {
  let result = u !== null ? u : undefined;
  if (t) {
    result = t;
  }
  return result;
}

function pick<T, U>(t: T, u: U): U | null {
  if (u !== null) {
    let result = u;
    if (typeof t === "number") {
      result = t;
    }
    return result;
  }
  return null;
}

function main(): void {
  console.log(swap(1, "a"), pick(1, "b"));
}
