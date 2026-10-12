// A spread nested inside a spread source takes the outer literal's context
// too, so its fields keep the literal types the expected type names. A field
// a later write overwrites is left free, so it may hold another type first.
type Switch = { kind: "on" | "off"; tag: number; label: string };

function make(flag: boolean): Switch {
  return { ...{ tag: 1, ...{ kind: flag ? "on" : "off", label: 0 } }, label: "x" };
}

function main(): void {
  const on = make(true);
  assert(on.kind === "on" && on.tag === 1 && on.label === "x", "the on switch");
  assert(make(false).kind === "off", "the off switch");
}
