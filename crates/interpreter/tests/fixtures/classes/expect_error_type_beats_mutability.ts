// A member that is both wrongly typed and unwritable reports the type: it subsumes
// the mutability problem, and a `readonly` fix would send the reader after the
// smaller of the two.
// expect-error: member `area` has an incompatible signature
// expect-error: `Base.area` is `string`, but `Shape` declares it as `number`
interface Shape {
  area: number;
}

class Base implements Shape {
  readonly area: string = "x";
}

function main(): void {
  const s: Shape = new Base();
  console.log(`${s.area}`);
}
