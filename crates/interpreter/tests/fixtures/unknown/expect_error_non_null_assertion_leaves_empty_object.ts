// expect-error: field `length` does not exist on `{}`

function main(): void {
  const x: unknown = "hello";
  const n = x!.length;
}
