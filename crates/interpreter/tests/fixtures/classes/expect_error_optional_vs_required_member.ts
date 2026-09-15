// An optional class member against a required interface property renders the same
// type on both sides, so the diagnostic has to name the optionality.
// expect-error: member `area` is optional
// expect-error: drop the `?` on the class
interface Shape {
  area: number;
}

class Base implements Shape {
  area?: number;
}

function main(): void {
  const s: Shape = new Base();
  console.log(`${s.area}`);
}
