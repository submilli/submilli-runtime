// A binding of `typeof x` holds only the tags `typeof` returns.
// expect-error: "strnig"
// expect-error-count: 1
function main(): void {
  const t = 10;
  let tag = typeof t;
  tag = "strnig";
}
