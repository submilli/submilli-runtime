// A static member named through a subclass reads the subclass, so using it
// before the subclass's declaration has run throws, as in JavaScript, even
// though the member is the parent's.
class Base {
  static count: number = 5;
  static hello(): string { return "hi"; }
  static greet: () => string = () => "greet";
}

function attempt(use: () => string): string {
  try {
    return use();
  } catch (e) {
    return (e as Error).message;
  }
}

const early = [
  attempt(() => Sub.hello()),
  attempt(() => String(Sub.count)),
  attempt(() => Sub.greet()),
  attempt(() => {
    const hello = Sub.hello;
    return hello();
  }),
  attempt(() => Base.hello()),
];

class Sub extends Base {}

function main(): void {
  const refused = "Cannot access 'Sub' before initialization";
  assert(early[0] === refused, "a static method through the subclass");
  assert(early[1] === refused, "a static field through the subclass");
  assert(early[2] === refused, "a function-typed static field through the subclass");
  assert(early[3] === refused, "a static method read as a value");
  assert(early[4] === "hi", "the parent itself is declared");
  assert(Sub.hello() === "hi" && Sub.count === 5 && Sub.greet() === "greet", "after the declaration");
}
