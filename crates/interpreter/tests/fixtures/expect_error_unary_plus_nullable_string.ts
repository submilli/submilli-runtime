// The operand shape most likely to be written by mistake. It is rejected —
// `null` has no numeric coercion — but the help still names the conversion,
// which is the whole point of reaching for `+s` in the first place.
// expect-error: unary `+` not defined for `string | null`
// expect-error: convert first: `Number(s)`
function main(): void {
  const maybe: string | null = "6" as string | null;
  const n = +maybe;
}
