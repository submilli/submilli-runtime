// expect-warning: `??` on non-nullable type
// A left side that is *only* `null` always takes the right side. Running it
// through `strip_null` instead yields the `Error` poison type, which carries no
// diagnostic and panics `value_type` at codegen — so the result has to be the
// right side's type outright. The alias spelling has to reach the same place.
type Nil = null;

function main(): void {
  const bare: null = null;
  assert((bare ?? "i") === "i", "bare null left side");

  const aliased: Nil = null;
  assert((aliased ?? "j") === "j", "aliased null left side");

  // Still the ordinary path for a left side that can hold a value.
  const empty: string | null = null;
  const held: string | null = "b";
  assert((empty ?? "k") === "k", "nullable left side holding null");
  assert((held ?? "l") === "b", "nullable left side holding a value");

  // The redundant-`??` warning still fires on a genuinely non-nullable left.
  const sure: string = "s";
  assert((sure ?? "m") === "s", "non-nullable left side");
}
