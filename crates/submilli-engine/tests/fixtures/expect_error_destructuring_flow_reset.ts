// expect-error: cannot read field `length` on non-object type `null`
function main(): number {
  const source = { x: "ok" };
  let { x }: { x: string | null } = source;
  x = null;
  return x.length;
}
