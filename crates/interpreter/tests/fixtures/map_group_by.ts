// SUB-1821: Map.groupBy groups by the callback's result, keeping keys of any
// type, in first-seen order, as in tsc's lib and JavaScript.
function main(): void {
  const byParity = Map.groupBy([1, 2, 3, 4, 5], (n) => n % 2 === 0);
  assert(byParity.size === 2);
  assert(byParity.get(false)!.join(",") === "1,3,5");
  assert(byParity.get(true)!.join(",") === "2,4");
  const keys: boolean[] = [];
  for (const key of byParity.keys()) {
    keys.push(key);
  }
  assert(keys.join(",") === "false,true");
  const words = ["apple", "avocado", "banana", "cherry", "blueberry"];
  const byLetter: Map<string, string[]> = Map.groupBy(words, (word) => word.charAt(0));
  assert(byLetter.get("b")!.join(",") === "banana,blueberry");
  const indexes = Map.groupBy(new Set(["x", "y", "z"]), (_value, index) => index < 1);
  assert(indexes.get(true)!.join("") === "x" && indexes.get(false)!.join("") === "yz");
  assert(Map.groupBy([] as number[], (n) => n).size === 0);
}
