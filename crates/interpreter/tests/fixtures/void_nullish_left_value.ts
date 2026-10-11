function nothing(): void {}
function number(): number { return 1; }
function main(): void {
  const key: string = ["a"][0];
  assert((nothing() ?? nothing()) === undefined, "coalesce two void values");
  const value = (key === "a" ? nothing() : number()) ?? 3;
  assert(value === 3, "conditional void branch coalesces to fallback");
}
