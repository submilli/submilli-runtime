// A loop's condition and `for` update run on every pass that reaches the back
// edge, a `continue` included, so they keep the narrowings in force there even
// when the body ends in `return`.
function whileLoop(x: string | null): number {
  if (x === null) return -1;
  let i = 0;
  while (x.length > i) {
    i++;
    if (i < 3) continue;
    return i;
  }
  return i;
}

function forUpdate(x: string | null): number {
  if (x === null) return -1;
  let total = 0;
  for (let i = 0; i < 3; i = i + x.length) {
    total++;
    if (total < 2) continue;
    return total;
  }
  return total;
}

function doWhile(x: string | null): number {
  if (x === null) return -1;
  let i = 0;
  do {
    i++;
    if (i < 3) continue;
    return i;
  } while (x.length > i);
  return -2;
}

function main(): void {
  assert(whileLoop("abcd") === 3);
  assert(forUpdate("a") === 2);
  assert(doWhile("abcd") === 3);
  console.log("ok");
}
