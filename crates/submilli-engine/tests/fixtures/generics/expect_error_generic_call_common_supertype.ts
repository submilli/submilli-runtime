// An argument that neither fits the other candidates nor is their supertype is
// rejected, as in tsc. An object literal does not widen what a non-literal
// argument inferred: it is checked against it. An explicit type argument is
// not a candidate, so nothing widens it.
// expect-error: object literal is missing required field `bark` of type `Dog`
// expect-error: expected `Dog`, got `Animal`
// expect-error: expected `1`, got `"s"`
// expect-error: field `bark` does not exist on `Animal`
// expect-error-count: 4
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
  const a: Animal = { name: "a" };
  const lit = pick(dog, { name: "z" });
  const explicit = pick<Dog>(dog, a);
  const n = pick(1, "s");
  const widened = pick(dog, a);
  console.log(widened.bark);
}
