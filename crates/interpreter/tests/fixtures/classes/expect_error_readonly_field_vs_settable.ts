// The `readonly`-field form of the same mutability mismatch the getter-only case
// hits: both sides are `number`, so only naming the writability names a fix.
// expect-error: member `area` is not writable
// expect-error: make the class member writable
interface Shape {
  area: number;
}

class Base implements Shape {
  readonly area: number = 1;
}

function main(): void {
  const s: Shape = new Base();
  console.log(`${s.area}`);
}
