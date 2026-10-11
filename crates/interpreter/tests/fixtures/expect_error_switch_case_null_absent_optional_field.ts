// expect-error: does not return a value on all paths
// expect-error-count: 1
// An absent optional field is `undefined` in JavaScript, which `case null`
// doesn't match, so these cases don't cover every value (tsc: TS2366).
enum E {
  A,
  B,
}

function tagName(o: { tag?: E }): string {
  switch (o.tag) {
    case E.A:
      return "a";
    case E.B:
      return "b";
    case null:
      return "null";
  }
}

function main(): void {
  console.log(tagName({ tag: E.A }));
}
