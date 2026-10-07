// An array literal's element type doesn't depend on which element comes first:
// a `new` of a non-generic class types itself, and a conditional choosing
// between object literals normalizes with the other literals.
class Animal {
  name: string;
  constructor(name: string) {
    this.name = name;
  }
}
class Dog extends Animal {
  bark(): string {
    return "woof";
  }
}
class Box<T> {
  value: T;
  constructor(value: T) {
    this.value = value;
  }
}

function main(): void {
  const pets = [new Dog("rex"), new Animal("cat")];
  assert(pets[1].name === "cat", "a later base-class `new` widens the element type");

  const boxes = [new Box(1), new Box<number>(2)];
  assert(boxes[0].value + boxes[1].value === 3, "generic classes still infer");

  const big = [1].length > 0;
  const chosen = [{ a: 1 }, big ? { a: 2, b: 10 } : { a: 3 }];
  let total = 0;
  for (const item of chosen) total += item.a + (item.b ?? 0);
  assert(total === 13, "a conditional of object literals normalizes");

  const first = [big ? { a: 2, b: "s" } : { a: 3 }, { a: 1, d: true }];
  assert(first[0].b === "s" && first[1].d === true, "the conditional may come first");
}
