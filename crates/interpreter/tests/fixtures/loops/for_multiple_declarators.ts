// A `for` head may declare several variables. Each `let` gets a fresh binding per
// iteration (ECMA-262 §14.7.4.4), and the update may hold several expressions.
function main(): void {
  const pairs: Array<() => string> = [];
  for (let i = 0, j = 3; i < j; i++, j--) {
    pairs.push(() => `${i}:${j}`);
  }
  assert(pairs.map((f: () => string): string => f()).join(" ") === "0:3 1:2", "each closure keeps its own iteration's `i` and `j`");

  const xs = [1, 2, 3, 4, 5];
  for (let lo = 0, hi = xs.length - 1; lo < hi; lo++, hi--) {
    const t = xs[lo];
    xs[lo] = xs[hi];
    xs[hi] = t;
  }
  assert(xs.join(",") === "5,4,3,2,1", "two indexes walk toward each other");

  let total = 0;
  for (let i = 0, n = xs.length; i < n; i++) {
    total += xs[i];
  }
  assert(total === 15, "a cached length beside the counter");

  let steps = 0;
  for (const limit = 2, start = 0; steps < limit + start; ) {
    steps++;
  }
  assert(steps === 2, "a `const` head with several declarators");

  let k = 0, m = 0;
  for (; k < 3; k++, m += 2) {}
  assert(k === 3 && m === 6, "a comma update without a declaration head");

  for (k = 10, m = 0; k > m; k--, m++) {}
  assert(k === 5 && m === 5, "an initializer may assign several variables");

  const order: string[] = [];
  for (let a = log(order, "a"), b = log(order, "b"); false; ) {}
  assert(order.join("") === "ab", "declarators run in source order");
}

function log(order: string[], name: string): number {
  order.push(name);
  return 0;
}
