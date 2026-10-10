// A getter-only member against a settable interface property is a mutability
// mismatch, not a type mismatch — the message must not name the same type twice.
// expect-error: member `area` is not writable
// expect-error: set area(value: number)
// expect-error: declare the interface property `readonly area: number`
interface Shape {
  area: number;
}

class Base implements Shape {
  get area(): number {
    return 1;
  }
}

function main(): void {
  const s: Shape = new Base();
  console.log(`${s.area}`);
}
