// An argument's literals take their literal types after a generic call is
// inferred, but the collection still holds the type argument it bound.
function keep<T>(xs: T[]): T[] {
  return xs;
}

function main(): void {
  const flags = keep([true]);
  flags.push(false);
  assert(flags.length === 2 && flags[1] === false, "keep([true]) holds booleans");
  const m = new Map([["k", true]]);
  m.set("q", false);
  assert(m.get("q") === false && m.size === 2, "the map holds booleans");
}
