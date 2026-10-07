// A bare literal `case` names no enum member, as in TypeScript, so cases
// spelled `0` and `1` don't match every value of `E`.
// expect-error: function `numbered` does not return a value on all paths
// expect-error: function `lettered` does not return a value on all paths
// expect-error-count: 2
enum E {
  A,
  B,
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

function main(): void {
  console.log(numbered(E.B), lettered(S.X));
}
