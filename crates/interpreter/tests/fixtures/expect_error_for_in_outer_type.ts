// expect-error: expected `number`
function main(): void {
  let n = 0;
  const o = { a: 1 };
  for (n in o) console.log(n);
}
