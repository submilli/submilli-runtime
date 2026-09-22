function copy<T>(value: T): [T, number] {
  const source: [T] = [value];
  return [...source, 2];
}
function main(): void {
  const generic = copy("generic");
  assert(generic[0] === "generic" && generic[1] === 2, "generic slots");
  const pair: [number, string] = [1, "two"];
  const same: [number, string] = [...pair];
  const grown: [boolean, number, string, number] = [true, ...pair, 3];
  const combined: [number, string, number, string] = [...pair, ...same];
  assert(combined[2] === 1 && combined[3] === "two", "multiple spreads");
  const erased: [unknown, unknown, unknown] = [...pair, false];
  assert(same[0] === 1 && same[1] === "two", "copy");
  assert(grown[0] && grown[1] === 1 && grown[2] === "two" && grown[3] === 3, "positions");
  assert(erased[0] === 1 && erased[2] === false, "erased slots");
}
