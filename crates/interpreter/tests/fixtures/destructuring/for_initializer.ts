// A destructuring pattern may initialize a `for` loop, at module level or in
// a function, with array or object patterns, `let` or `const`.
const pair: number[] = [0, 3];
let moduleSum = 0;
for (let [i, j] = pair; i < j; i++) moduleSum += i;

function main(): void {
  assert(moduleSum === 3, "a module-level array pattern");

  let seen = "";
  for (let { p, q } = { p: 1, q: 3 }; p < q; p++) seen += String(p);
  assert(seen === "12", "an object pattern");

  let total = 0;
  for (const [step, limit] = [2, 7]; total < limit; ) total += step;
  assert(total === 8, "a const pattern");

  let odd = 0;
  for (let [k] = [0]; k < 5; k++) {
    if (k % 2 === 0) continue;
    odd += k;
  }
  assert(odd === 4, "continue in a destructured loop");

  let tagged = "";
  for (let [a, b]: [number, string] = [0, "x"]; a < 2; a++) tagged += b + String(a);
  assert(tagged === "x0x1", "an annotated tuple pattern");

  let restSeen = "";
  for (let [h, ...rest] = [1, 2, 3]; h < 3; h++) restSeen += String(h) + String(rest.length);
  assert(restSeen === "1222", "a rest element");

  let calls = 0;
  const make = (): number[] => {
    calls++;
    return [0, 3];
  };
  for (let [from, to] = make(); from < to; from++) {}
  assert(calls === 1, "the initializer runs once");

  let nest = "";
  for (let [outer] = [0]; outer < 2; outer++) {
    for (let [inner] = [outer]; inner < 2; inner++) nest += String(outer) + String(inner);
  }
  assert(nest === "000111", "nested destructured loops");

  let broke = "";
  for (let [i, j] = [0, 3]; ; i++) {
    if (i >= j) break;
    broke += String(i);
  }
  assert(broke === "012", "an empty condition with break");

  let skipped = "";
  for (let [, j] = [9, 2]; j > 0; j--) skipped += String(j);
  assert(skipped === "21", "an elided element");

  let renamed = "";
  for (let { a: p, b: q } = { a: 1, b: 3 }; p < q; p++) renamed += String(p);
  assert(renamed === "12", "renamed object pattern properties");

  const tuple: [string, number] = ["a", 2];
  let repeated = "";
  for (let [s, n] = tuple; n > 0; n--) repeated += s;
  assert(repeated === "aa", "a tuple variable initializer");

  let shadowed = 0;
  for (let [i] = [0]; i < 2; i++) {
    let i = 5;
    shadowed += i;
  }
  assert(shadowed === 10, "the body may redeclare a pattern binding");

  // The bindings are scoped to the loop.
  const i = "outer";
  for (let [i] = [0]; i < 1; i++) {}
  assert(i === "outer", "the loop's bindings end with it");

  // A closure may capture a `const` pattern's bindings: they never change.
  const read: (() => number)[] = [];
  for (const { base } = { base: 10 }; read.length < 2; ) read.push(() => base);
  assert(read[1]() === 10, "a captured const binding");
}
