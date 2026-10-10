import { Animal, Dog } from "@test/zoo";

// The consumer reconstructs the imported `$Animal`/`$Dog` rec groups, so it can
// construct, dispatch, and `instanceof`-test them. It cannot see `Animal`'s
// private `sound`.
function main(): void {
  const rex = new Dog("Rex");
  assert(rex.speak() === "Rex barks");

  // Vtable dispatch through a base-typed reference still hits the override.
  const a: Animal = rex;
  assert(a.speak() === "Rex barks");
  assert(a instanceof Dog);

  const generic = new Animal("Critter", "moo");
  assert(generic.speak() === "Critter says moo");
  assert(!(generic instanceof Dog));
}
