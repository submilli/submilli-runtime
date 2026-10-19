// expect-error: expected `{ describe: (arg0: Animal) => string }`
// A function-typed property compares its parameters contravariantly, so a
// handler for `Dog` doesn't stand for one that takes any `Animal`.
class Animal {
  name: string = "animal";
}
class Dog extends Animal {
  bark(): string {
    return "woof";
  }
}
function main(): void {
  const narrow: { describe: (d: Dog) => string } = { describe: (d: Dog): string => d.bark() };
  const wide: { describe: (a: Animal) => string } = narrow;
  console.log(wide.describe(new Animal()));
}
