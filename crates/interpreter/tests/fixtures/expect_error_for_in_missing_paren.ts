// expect-error: expected `)` after `for…in` object
function main(): void {
  const o = { a: 1 };
  for (const k in o {
    console.log(k);
  }
}
