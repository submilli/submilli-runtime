// The literal a function literal called on the spot returns widens even when
// the call's context names its literals through an alias, as in tsc.
// expect-error: expected `M`, got `string`
// expect-error-count: 1
type M = "a" | "b";

function main(): void {
  const m: M = (() => "a")();
  console.log(m);
}
