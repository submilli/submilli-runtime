// A method is fixed on its class, but a view of the instance whose member is a
// function-typed property may replace it. A direct class-typed call sees the
// replacement, as in TypeScript; a method nothing can rewrite keeps the
// vtable call.
interface Swappable { greet: () => string; }
class Greeter {
  greet(): string { return "hello"; }
  fixed(): string { return "fixed"; }
}
function main(): void {
  const g = new Greeter();
  assert(g.greet() === "hello", "the declared method runs first");
  const view: Swappable = g;
  view.greet = (): string => "swapped";
  assert(g.greet() === "swapped", "a class-typed call sees the rewrite");
  assert(view.greet() === "swapped", "and so does the alias");
  assert(g.fixed() === "fixed", "an unwritten method is unchanged");
}
