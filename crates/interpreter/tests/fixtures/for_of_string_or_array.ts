// A union of a string with arrays or tuples iterates as whichever it holds: a
// string by code point, an array by element. Its `length` is the string's or
// the array's.
function collect(source: string | number[]): string {
  const seen: string[] = [];
  for (const item of source) {
    seen.push(String(item));
  }
  return seen.join(",");
}

function joinParts(pair: string | [number, string]): string {
  let joined = "";
  for (const part of pair) {
    joined += String(part);
  }
  return joined;
}

function countBeforeY(letters: "xy" | "z" | string[]): number {
  let count = 0;
  for (const letter of letters) {
    if (letter === "y") break;
    count++;
  }
  return count;
}

function stepsWhileGrowing(growing: string | number[]): number {
  let steps = 0;
  for (const _ of growing) {
    if (typeof growing !== "string" && growing.length < 3) growing.push(growing.length + 1);
    steps++;
  }
  return steps;
}

function lengthOf(value: string | number[]): number {
  return value.length;
}

let reads = 0;
function counted(value: string | string[]): string | string[] {
  reads++;
  return value;
}

function main(): void {
  assert(collect("ab") === "a,b", "a string iterates its characters");
  assert(collect([1, 2]) === "1,2", "an array iterates its elements");
  assert(collect("a😀") === "a,😀", "a string iterates by code point");
  assert(joinParts([3, "c"]) === "3c", "a tuple member iterates its positions");
  assert(joinParts("de") === "de", "the string member of a tuple union");
  assert(countBeforeY("xy") === 1, "break leaves the loop");
  assert(countBeforeY(["a", "y", "b"]) === 1, "break leaves the array loop");
  assert(stepsWhileGrowing([1]) === 3, "the loop sees elements pushed while it runs");
  assert(lengthOf("four") === 4 && lengthOf([1, 2]) === 2, "length of either member");
  assert(lengthOf("a😀") === 3, "a string's length counts UTF-16 units");
  assert(counted(["p", "q"]).length === 2 && reads === 1, "the receiver is read once");
}
