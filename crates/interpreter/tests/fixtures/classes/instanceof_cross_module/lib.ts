// `instanceof` resolves against an imported class: the consumer holds `$Dog` in
// its imported rec group, so `ref.test (ref $Dog)` is well-typed cross-module
// (docs/classes.md §9–§10). Multi-file fixtures are typecheck-only.
import { Animal, Dog } from "./animals";

// A module-local helper (not exported) — its `Animal` parameter is fine because
// the type isn't crossing the module's public surface.
function describe(animal: Animal): string {
  if (animal instanceof Dog) {
    // Narrowed to the imported `Dog`; `learn` is only on Dog.
    return animal.learn("sit");
  }
  return animal.speak();
}

export function run(): string {
  const rex: Animal = new Dog("Rex");
  const generic: Animal = new Animal("Critter");
  return describe(rex) + " / " + describe(generic);
}
