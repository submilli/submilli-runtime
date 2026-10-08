// Comparing a union of bigint literals with `===`, `!==` or a `switch` narrows
// it like a union of number literals, as in TypeScript.

type Small = 1n | 2n;

function pick(x: bigint): Small {
  return x > 1n ? 2n : 1n;
}

function label(s: Small): string {
  if (s !== 1n) {
    const two: 2n = s;
    return two === 2n ? "two" : "?";
  }
  const one: 1n = s;
  return one === 1n ? "one" : "?";
}

function viaSwitch(s: Small): string {
  switch (s) {
    case 1n:
      return "one";
    default: {
      const two: 2n = s;
      return two === 2n ? "two" : "?";
    }
  }
}

function main(): void {
  const small = pick(5n);
  if (small === 2n) {
    const two: 2n = small;
    assert(two === 2n, "`===` narrows a bigint literal union");
  }
  assert(label(1n) === "one" && label(2n) === "two", "`!==` narrows both ways");
  assert(viaSwitch(1n) === "one" && viaSwitch(2n) === "two", "a `case` narrows the default");
  console.log("ok");
}
