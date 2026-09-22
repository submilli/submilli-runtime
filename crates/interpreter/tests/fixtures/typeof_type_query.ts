// `typeof x` names the type of a value, so a declaration can track another's type
// without restating it. Composes with `[]` and with a dotted path.
const base = 1;
let mutable = "hi";
function transform(n: number): boolean { return n > 0; }
const config = { retries: 2 };

function main(): void {
  const a: typeof base = 5;
  const b: typeof mutable = "other";
  const f: typeof transform = transform;
  const c: typeof config = { retries: 9 };
  const xs: typeof base[] = [1, 2];
  const r: typeof config.retries = 3;

  assert(a === 5, "value");
  assert(b === "other", "let");
  assert(f(1), "function");
  assert(c.retries === 9, "object");
  assert(xs.length === 2, "array of");
  assert(r === 3, "dotted path");
}
