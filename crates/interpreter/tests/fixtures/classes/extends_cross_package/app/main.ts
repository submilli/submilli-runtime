import { Animal } from "@test/animals";
import { Dog, Cat } from "@test/dogs";

function main(): void {
  const rex = new Dog("Rex");
  assert(rex.speak() === "Rex barks"); // override, vtable dispatch
  assert(rex.describe() === "an animal named Rex"); // inherited body (other pkg)
  assert(rex.learn() === 1); // own method + own field
  assert(rex.learn() === 2);
  assert(rex.name === "Rex"); // inherited public field

  // Vtable dispatch through a base-typed reference still hits the override, and
  // the inherited method still reaches Animal's body.
  const a: Animal = rex;
  assert(a.speak() === "Rex barks");
  assert(a.describe() === "an animal named Rex");
  assert(a instanceof Dog);

  const generic = new Animal("Critter", "moo");
  assert(generic.speak() === "Critter says moo");
  assert(generic.describe() === "an animal named Critter");
  assert(!(generic instanceof Dog));

  // `Cat` has no constructor — it inherits Animal's `(name, sound)` signature.
  const tom = new Cat("Tom", "meow");
  assert(tom.speak() === "Tom says meow");
  assert(tom.describe() === "an animal named Tom");
  assert(tom instanceof Cat);
  const tomAnimal: Animal = tom;
  assert(tomAnimal instanceof Cat);
}
