// A guard that rules out every value a local can hold makes it `never` where
// the guard holds, as in TypeScript, so code there can hand it to a function
// that takes `never`. That code never runs. A type parameter or `unknown`
// can hold values a guard doesn't name, so it stays as it is.
function unreachable(value: never): string {
  return "unreachable";
}

function nestedTruthiness(flag: boolean): string {
  if (flag) {
    if (flag) {
      return "true";
    }
    return unreachable(flag);
  }
  return "false";
}

function nestedTypeof(value: string | number): string {
  if (typeof value === "number") {
    if (typeof value !== "number") {
      return unreachable(value);
    }
    return (value + 1).toString();
  }
  return value;
}

function repeatedNullCheck(items: (string | null)[]): number {
  let total = 0;
  for (const item of items) {
    if (item === null) {
      continue;
    }
    if (item === null) {
      return total + unreachable(item).length;
    }
    total += item.length;
  }
  return total;
}

function generic<T>(value: T): string {
  if (typeof value === "object") {
    return "object";
  }
  return "other";
}

function main(): void {
  console.log(nestedTruthiness(true), nestedTruthiness(false));
  console.log(nestedTypeof(1), nestedTypeof("s"));
  console.log(repeatedNullCheck(["ab", null, "c"]));
  console.log(generic(1), generic("s"));
}
