// An array literal whose elements all fit one another holds tsc's best common
// type: the union of their types, less each one that is a subtype of another.
type WithY = { x: number; y?: number };
type WithZ = { x: number; z?: number };
type Options = { a?: number };

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

function sumX(items: (WithY | WithZ)[]): number {
  let total = 0;
  for (const item of items) total += item.x;
  return total;
}

function main(): void {
  const y: WithY = { x: 1, y: 2 };
  const z: WithZ = { x: 3, z: 4 };
  // Neither type has the other's optional field, so the array holds both.
  const both = [y, z];
  both.push({ x: 5 });
  assert(sumX(both) === 9, "the array holds either type");

  // An empty object literal is a subtype of a type of optional fields.
  const opts: Options = { a: 6 };
  const withEmpty = [{}, opts];
  assert(withEmpty[1].a === 6 && (withEmpty[0].a ?? 0) === 0, "reads through the wider type");

  // A subclass instance is a subtype of its base.
  const dog = new Dog("rex");
  const animal: Animal = new Animal("cat");
  const pets = [dog, animal];
  assert(pets[1].name === "cat", "the array holds the base class");
}
