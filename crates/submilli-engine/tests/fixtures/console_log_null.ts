// `console.log` accepts any value, so a nullable one holding `null` typechecks. It
// prints `null`, as in JavaScript, rather than reaching for a `toString` that `null`
// has no vtable to provide.
function main(): void {
  const n: number | null = null;
  console.log(n);
  console.log("before", n, "after");
  const items: string[] | null = null;
  console.log(items);
}
