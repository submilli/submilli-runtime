// expect-error: `v` is `seen[key]`: reading a record by key gives `undefined` when the key is absent
// expect-error: guard first — `if (v !== undefined) { … v.x … }` — or read it as `v?.x`, or assert non-null with `v!.x`
function main(): void {
  const seen: Record<string, { x: number }> = {};
  for (const key of Object.keys(seen)) {
    const v = seen[key];
    console.log(v.x);
  }
}
