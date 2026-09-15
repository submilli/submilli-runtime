// expect-error: cannot read field `length` on `string | null`: the receiver can be `null`
// The read runs before the assignment that would narrow `x`, so iteration 1
// sees `null`. The same program without the `continue` reports this too — the
// explicit back edge must not buy the read a narrowing.
function run(x: string | null): number {
  let n = 0;
  let len = 0;
  while (n < 2) {
    len = len + x.length;
    n = n + 1;
    x = "zz";
    continue;
  }
  return len;
}

function main(): void {
  assert(run(null) === 0, "unreachable — the program does not compile");
}
