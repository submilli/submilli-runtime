// expect-error: expected `{ g: number }`
function main(): void {
  const result: { g: number } = Object.assign({ g: "wrong" }, { other: 1 });
}
