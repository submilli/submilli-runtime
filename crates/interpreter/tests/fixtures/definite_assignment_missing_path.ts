// expect-error: variable `value` is used before being assigned
function main(): void {
  let value: number;
  try { value = 1; } catch {}
  console.log(value);
}
