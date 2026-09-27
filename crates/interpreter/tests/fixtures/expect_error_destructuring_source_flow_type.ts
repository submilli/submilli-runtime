// expect-error: expected `string`, got `null`
function main(): void {
  const source: { x: string | null } = { x: null };
  source.x = "ok";
  let { x }: { x: string | null } = source;
  x = null;
}
