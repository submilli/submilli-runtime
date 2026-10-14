// A method member of an object type compares its parameters bivariantly, as
// in tsc, whether the type is an alias, an anonymous object type, or an
// interface; a function-typed property still compares them contravariantly.
class Animal {
  name: string = "animal";
}
class Dog extends Animal {
  bark(): string {
    return "woof";
  }
}
type Sink<T> = { put(x: T): number };
interface InterfaceSink<T> {
  put(x: T): number;
}

function main(): void {
  const dogs: Sink<Dog> = { put: (d: Dog): number => d.bark().length };
  const animals: Sink<Animal> = dogs;
  const narrow: { describe(d: Dog): string } = { describe: (d: Dog): string => d.bark() };
  const wide: { describe(a: Animal): string } = narrow;
  const interfaceDogs: InterfaceSink<Dog> = { put: (d: Dog): number => d.bark().length };
  const interfaceAnimals: InterfaceSink<Animal> = interfaceDogs;
  assert(animals.put(new Dog()) === 4);
  assert(wide.describe(new Dog()) === "woof");
  assert(interfaceAnimals.put(new Dog()) === 4);
}
