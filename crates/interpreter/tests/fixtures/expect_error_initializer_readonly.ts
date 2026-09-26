// expect-error: readonly
function main(): void {
  const value: { readonly n: number } | null = { n: 1 };
  value.n = 2;
}
