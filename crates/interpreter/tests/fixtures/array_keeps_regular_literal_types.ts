// An array literal keeps the regular literal types of its elements, as
// TypeScript does: `[h]` with `h: "hello"` is `"hello"[]`. Fresh ones widen,
// so `[c, h]` with `const c = "c"` is `string[]`.
function hello(h: "hello"): number {
  return h.length;
}
function small(n: 1 | 2): number {
  return n;
}

function build(h: "hello", n: 1 | 2): number {
  const fresh = "c";
  const greetings = [h, h];
  let sizes = [n];
  sizes.push(2);
  const nested = [[h]];
  const mixed = [fresh, h];
  mixed.push("other");
  let total = 0;
  for (const greeting of greetings) {
    total += hello(greeting);
  }
  return total + small(sizes[1]) + hello(nested[0][0]) + mixed.length;
}

function main(): void {
  assert(build("hello", 1) === 10 + 2 + 5 + 3, "element literal types reach literal parameters");
}
