// A defaulted binding of an unannotated pattern is mutable, so its default's
// literal widens, as in tsc: `x = 0` binds `number`. A pattern with a written
// type binds its member's type, literals included.
function bump([x = 0, label = "n"] = []): string {
  x = x + 5;
  label = label + "!";
  return label + String(x);
}

type Mode = "fast" | "slow";

function main(): void {
  assert(bump() === "n!5", "parameter defaults widen");
  let { mode = "fast" }: { mode?: Mode } = {};
  const m: Mode = mode;
  mode = "slow";
  assert(m === "fast" && mode === "slow", "an annotated pattern keeps its member's literals");
  let [count = 1] = [];
  count = 3;
  assert(count === 3, "a `let` pattern's default widens");
}
