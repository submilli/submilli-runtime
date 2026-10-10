// expect-error: `1.` is a complete number, so a name cannot follow it directly
// expect-error: `0.` is a complete number, so a name cannot follow it directly
// expect-error-count: 3
function main(): void {
  console.log(1.toString());
  console.log(0.toFixed(1));
  console.log(1_000.toString());
}
