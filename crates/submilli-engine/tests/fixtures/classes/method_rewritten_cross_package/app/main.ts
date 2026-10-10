// The importing module's write through a structural view reaches an instance
// of an imported class, so a class-typed call in this module reads the
// replacement, as a read through the view does.
import { Greeter } from "@test/greeter";
interface Swappable { greet: () => string; }
function main(): void {
  const g = new Greeter();
  assert(g.greet() === "hello", "the declared method runs first");
  const view: Swappable = g;
  view.greet = (): string => "swapped";
  assert(g.greet() === "swapped", "a class-typed call sees the rewrite");
  assert(view.greet() === "swapped", "and agrees with the view");
  assert(g.fixed() === "fixed", "an unwritten method is unchanged");
}
