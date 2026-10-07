// A `default` after cases naming every member of `S` still holds `WithEnum`,
// whose `tag` can be `"p"`, the value `WithLiteral`'s cases leave, as in
// TypeScript.
// expect-error: field `d` does not exist on all members of `WithEnum | WithLiteral`
// expect-error-count: 1
enum S {
  P = "p",
  Q = "q",
}
interface WithEnum {
  tag: S;
  c: number;
}
interface WithLiteral {
  tag: "p";
  d: string;
}
function describe(u: WithEnum | WithLiteral): string {
  switch (u.tag) {
    case S.P:
    case S.Q:
      return "s";
    default:
      return u.d;
  }
}
function main(): void {
  console.log(describe({ tag: "p", d: "x" }));
}
