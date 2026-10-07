// A bare literal `case` names no enum member, and a member names only its
// own enum's value, as in TypeScript: cases spelled `0` and `1` don't match
// every value of `E`, and cases `E.A` and `E.B` don't match `0` or `F.X`.
// expect-error: function `numbered` does not return a value on all paths
// expect-error: function `lettered` does not return a value on all paths
// expect-error: function `withLiteral` does not return a value on all paths
// expect-error: function `withOtherEnum` does not return a value on all paths
// expect-error-count: 4
enum E {
  A,
  B,
}
enum F {
  X,
  Y,
}
enum S {
  X = "x",
  Y = "y",
}

function numbered(e: E): number {
  switch (e) {
    case 0:
      return 10;
    case 1:
      return 20;
  }
}

function lettered(s: S): number {
  switch (s) {
    case "x":
      return 1;
    case "y":
      return 2;
  }
}

function withLiteral(v: E | 0): number {
  switch (v) {
    case E.A:
      return 10;
    case E.B:
      return 20;
  }
}

function withOtherEnum(v: E | F): number {
  switch (v) {
    case E.A:
      return 10;
    case E.B:
      return 20;
  }
}

function main(): void {
  console.log(numbered(E.B), lettered(S.X), withLiteral(0), withOtherEnum(F.X));
}
