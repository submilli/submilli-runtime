function main(): void {
  let people: Record<string,string>={};
  people["name"] = "Ada";
  assert(people["name"] === "Ada");
  const nested: Array<Array<number>>=[[3]];
  assert(nested[0][0] === 3);
  let n = 8;
  n >>= 1;
  assert(n >= 4);
  assert(n >> 1 === 2);
}
