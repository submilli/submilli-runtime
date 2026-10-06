// A switch with no `default` falls through to the code after it whenever a
// case is left unmatched or a matched case breaks, whether the cases name a
// literal union's members or `null` beside them.
type MaybeKind = "a" | "b" | null;
type Kind = "a" | "b";

function withNullCase(kind: MaybeKind): number {
  switch (kind) {
    case "a": break;
    case "b": return 2;
    case null: return 3;
  }
  return 1;
}

function everyCaseReturnsButOne(kind: Kind): number {
  switch (kind) {
    case "a": break;
    case "b": return 2;
  }
  return 1;
}

function everyCaseBreaks(kind: Kind): number {
  switch (kind) {
    case "a": break;
    case "b": break;
  }
  return 1;
}

function main(): void {
  console.log(withNullCase("a"), withNullCase("b"), withNullCase(null));
  console.log(everyCaseReturnsButOne("a"), everyCaseReturnsButOne("b"));
  console.log(everyCaseBreaks("b"));
}
