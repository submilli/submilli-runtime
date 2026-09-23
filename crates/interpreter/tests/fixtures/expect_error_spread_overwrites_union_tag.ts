// A spread after the tag can overwrite it. Its field is checked against the
// variant the tag selected, as TypeScript rejects it (TS2322): the spread
// object's `kind` widened to `string`.
// expect-error: spread field `kind`: expected `"circle"`, got `string`

interface Circle {
  kind: "circle";
  radius: number;
}

interface Square {
  kind: "square";
  side: number;
}

function main(): void {
  const other = { kind: "square", side: 1, radius: 1 };
  const s: Circle | Square = { kind: "circle", ...other };
  console.log(s.kind);
}
