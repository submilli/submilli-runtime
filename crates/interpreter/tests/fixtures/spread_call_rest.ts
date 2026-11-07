function sum(...values: number[]): number {
  let total = 0;
  for (const value of values) total += value;
  return total;
}

function join(...values: string[]): string {
  return values.join(":");
}

function collect<T>(...values: T[]): T[] {
  return values;
}

function collectAfter<T>(head: number, ...values: T[]): T[] {
  return values;
}

function main(): void {
  const page = [1, 2, 3];
  const all: number[] = [];
  all.push(0, ...page, 4);
  assert(all.join(",") === "0,1,2,3,4", "mixed spread in push");
  let inferred = [];
  inferred.push(...page);
  assert(inferred.join(",") === "1,2,3", "spread into inferred growing array");
  assert(Math.max(...page) === 3, "spread in Math.max");
  assert(sum(1, ...page, 4) === 11, "mixed spread in user rest call");
  assert(collect(...page).join(",") === "1,2,3", "generic rest call");
  assert(sum(...new Set<number>([2, 3])) === 5, "set spread");
  const map = new Map<string, number>([["a", 1], ["b", 2]]);
  assert(join(...map.keys()) === "a:b", "map keys spread");
  assert(join(..."ab") === "a:b", "string spread");
  let changing: number[] | string[] = [1];
  const after: string[] = collectAfter((changing = ["a"]).length, ...changing);
  assert(after[0] === "a", "earlier argument narrows a later spread");
  const front = [3];
  front.unshift(...page);
  assert(front.join(",") === "1,2,3,3", "unshift spread");
  assert([0].concat(...[[1], [2]]).join(",") === "0,1,2", "concat spread");
  console.log(...["spread", "log"]);
}
