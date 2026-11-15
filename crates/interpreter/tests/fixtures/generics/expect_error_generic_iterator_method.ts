// A generic `iterator` method would be called by `for…of` without type
// arguments, so it doesn't make a class iterable.
// expect-error: `for-of` requires an array, tuple, string, `Iterator<T>`, or `Iterable<T>`, got `Bag`
// expect-error-count: 1
class Bag {
  iterator<T>(): Iterator<T> {
    throw new Error("unused");
  }
}

function main(): void {
  for (const x of new Bag()) {
  }
}
