// A non-literal argument decides a type parameter over an object literal
// before it, as in tsc, which then reports the literal that doesn't fit:
// `T` is `Dog` here, and `{ name: "z" }` has no `bark`.
// expect-error: expected `Dog`, got `{ name: string }`
// expect-error-count: 1
interface Animal {
  name: string;
}

interface Dog extends Animal {
  bark: string;
}

function pick<T>(a: T, b: T): T {
  return b;
}

function main(): void {
  const dog: Dog = { name: "d", bark: "w" };
  pick({ name: "z" }, dog);
}
