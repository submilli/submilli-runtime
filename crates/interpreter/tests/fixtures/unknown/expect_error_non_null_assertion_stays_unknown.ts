// expect-error: cannot read field `length` on `unknown`

function main(): void {
  const x: unknown = "hello";
  const n = x!.length;
}
