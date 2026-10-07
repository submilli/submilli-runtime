// A `switch` inside a loop that assigns `null` to a narrowed local invalidates
// the narrowing at the loop's back edge, as an `if`/`else` does.
function run(): string {
  let s: string | null = "a";
  let out = "";
  for (let n = 0; n < 3; n++) {
    switch (n) {
      case 0:
        out += s ?? "N";
        break;
      default:
        s = null;
    }
    out += s === null ? "-" : s;
  }
  let i = 0;
  let t: string | null = "b";
  while (i < 2) {
    switch (i) {
      case 9:
        break;
      default:
        t = null;
    }
    i++;
  }
  return out + (t ?? "null");
}

function main(): void {
  const result = run();
  assert(result === "aa--null", "the null assignment is seen after the switch");
  console.log(result);
}
