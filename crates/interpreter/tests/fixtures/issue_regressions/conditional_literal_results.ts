function check(value: number | null, flag: boolean): void {
  const fallback = 10;
  const chosen = value === null ? fallback : value;
  const reversed = value !== null ? value : fallback;
  const coalesced = value ?? fallback;
  assert(chosen / 2 === reversed / 2, "both ternary orders support arithmetic");
  assert(coalesced >= 0 && coalesced === chosen, "nullish result supports ordering");
  const suffix = "x";
  const text = flag ? suffix : String(chosen);
  const nullable: string | null = flag ? null : String(chosen);
  const nonNull = nullable ?? suffix;
  assert(`${text}` === `${nonNull}`, "string branches support interpolation");
}
function main(): void {
  check(null, true);
  check(6, false);
}
