// The enum members a guard ruled out stay ruled out in a `switch` on the same
// value (its `default` and the code after it), after a type predicate, and on
// a field, so a chain of member checks still ends in `never`.
enum S {
  X = "x",
  Y = "y",
  Z = "z",
}

function never(value: never): string {
  return "never";
}

function isS(value: S | number): value is S {
  return typeof value === "string";
}

function afterSwitch(e: S): string {
  switch (e) {
    case S.X:
      return "X";
  }
  if (e === S.Y) return "Y";
  if (e === S.Z) return "Z";
  return never(e);
}

function inDefault(e: S): string {
  switch (e) {
    case S.X:
      return "X";
    default:
      if (e === S.Y) return "Y";
      if (e === S.Z) return "Z";
      return never(e);
  }
}

// A switch whose cases name only the members left is exhaustive.
function switchAfterGuard(e: S): string {
  if (e === S.X) return "X";
  switch (e) {
    case S.Y:
      return "Y";
    case S.Z:
      return "Z";
  }
}

function afterPredicate(e: S | number): string {
  if (e === S.X) return "X";
  if (!isS(e)) return "number";
  if (e === S.Y) return "Y";
  if (e === S.Z) return "Z";
  return never(e);
}

function onField(o: { e: S }): string {
  if (o.e === S.X) return "X";
  if (o.e === S.Y) return "Y";
  if (o.e === S.Z) return "Z";
  return never(o.e);
}

// A field read as `never` still reads the value an alias wrote behind the
// guard, as JavaScript does.
function show(value: never): string {
  return `got ${String(value)}`;
}

let alias: { e: S } = { e: S.Z };

function onFieldWrittenThroughAlias(o: { e: S }): string {
  if (o.e === S.X) return "X";
  if (o.e === S.Y) return "Y";
  alias.e = S.X;
  if (o.e === S.Z) return "Z";
  return show(o.e);
}

function main(): void {
  assert(afterSwitch(S.Z) === "Z");
  assert(inDefault(S.Y) === "Y");
  assert(switchAfterGuard(S.Z) === "Z");
  assert(afterPredicate(3) === "number");
  assert(afterPredicate(S.Z) === "Z");
  assert(onField({ e: S.Y }) === "Y");
  const o = { e: S.Z };
  alias = o;
  assert(onFieldWrittenThroughAlias(o) === "got x");
  console.log("ok");
}
