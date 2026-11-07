// expect-error: cannot read field `length` on `string | undefined`
function main(): void {
  const match = /(a)?b/.exec("b");
  if (match === null) throw new Error("missing match");
  const [whole, first] = match;
  const required: string = whole;
  console.log(required, first.length);
}
