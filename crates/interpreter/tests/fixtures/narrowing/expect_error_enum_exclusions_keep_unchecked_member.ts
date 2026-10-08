// expect-error: expected `never`, got `S`
// `S.Z` was never ruled out, so the field is not `never` at the end.
enum S {
  X = "x",
  Y = "y",
  Z = "z",
}

function never(value: never): string {
  return "never";
}

function onField(o: { e: S }): string {
  if (o.e === S.X) return "X";
  switch (o.e) {
    case S.Y:
      return "Y";
    default:
      return never(o.e);
  }
}

function main(): void {
  console.log(onField({ e: S.Z }));
}
