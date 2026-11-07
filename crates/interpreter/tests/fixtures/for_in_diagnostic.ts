// expect-error: `for…in` is not supported
// expect-error-count: 1
function main(): void {
  const obj = { key: "value" };
  for (const key in obj) console.log(key);
}
