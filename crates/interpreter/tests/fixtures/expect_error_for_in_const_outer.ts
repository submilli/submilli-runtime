// expect-error: cannot assign
function main(): void {
  const k = "";
  const o = { a: 1 };
  for (k in o) console.log(k);
}
