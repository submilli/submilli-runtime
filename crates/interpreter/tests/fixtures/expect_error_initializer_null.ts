// expect-error: always null
function main(): void {
  const value: { n: number } | null = null;
  console.log(value?.n);
}
