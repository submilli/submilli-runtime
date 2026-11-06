const globalValues = [];
globalValues.push(7);
const globalFirst: number = globalValues[0];
let globalResult;
globalResult = "global";
const globalText: string = globalResult;

function branch(flag: boolean): string[] {
  const values = [];
  if (flag) values.push("yes");
  return values;
}
function optionalTry(flag: boolean): number | undefined {
  let value;
  try {
    if (flag) throw new Error("stop");
    value = 7;
  } catch (error) {}
  return value;
}
function unknownValue(input: unknown, flag: boolean): unknown {
  let value;
  if (flag) value = input; else value = "fallback";
  return value;
}
function main(): void {
  assert(globalFirst === 7 && globalText === "global", "global flow");
  assert(branch(false).length === 0 && branch(true)[0] === "yes", "one-sided growth");
  assert(optionalTry(false) === 7 && optionalTry(true) === undefined, "try has an unassigned path");
  assert(unknownValue(3, true) === 3, "unknown is a valid inferred value");
  let count;
  count = 0;
  for (let i = 0; i < 3; i++) count = count + 1;
  assert(count === 3, "loop assignment");
  const values = [];
  let i = 0;
  while (i < 3) { values.push(i); i++; }
  const nums: number[] = values;
  assert(nums.join(",") === "0,1,2", "while growth");
  let reset = [];
  reset.push(1);
  reset = [];
  reset.push("fresh");
  const strings: string[] = reset;
  assert(strings[0] === "fresh", "reset evolution");
  const nested = [];
  nested.push(nested.push("s"));
  const nestedValues: (string | number)[] = nested;
  assert(nestedValues[0] === "s" && nestedValues[1] === 1, "argument growth is preserved");
  let replaced = [];
  replaced.push((replaced = ["s"]).length);
  const replacement: string[] = replaced;
  assert(replacement[0] === "s" && replacement.length === 1, "held receiver precedes arguments");
  let indexed = [];
  indexed.push(0);
  indexed[0] = (indexed = ["fresh"]).length;
  const indexedReplacement: string[] = indexed;
  assert(indexedReplacement[0] === "fresh", "index receiver precedes value");
  let resetInside = [];
  resetInside.push(1);
  resetInside.push((resetInside = []).length === 0 ? "x" : "x");
  const resetStrings: string[] = resetInside;
  assert(resetStrings.length === 0, "argument reset leaves a different receiver");
  const scalarLiteral: "kept" = "kept";
  let scalar;
  scalar = scalarLiteral;
  const exactScalar: "kept" = scalar;
  assert(exactScalar === "kept", "scalar assignments retain declared literals");
  const guarded = [];
  guarded.push(1);
  if (Array.isArray(guarded)) guarded.push("s");
  const guardedValues: (number | string)[] = guarded;
  assert(guardedValues.length === 2, "guards preserve evolution");
  const optional = [];
  optional?.push(1);
  optional.unshift?.(2);
  const optionalLength: string = optional?.push(3).toString();
  const optionalNumbers: number[] = optional;
  assert(optionalLength === "3" && optionalNumbers.join(",") === "2,1,3", "optional method growth");
  let uninitialized;
  assert(uninitialized === undefined, "initial undefined");
  uninitialized = [];
  uninitialized.push(2);
  const inferred: number[] = uninitialized;
  assert(inferred[0] === 2, "uninitialized becomes evolving array");
}
