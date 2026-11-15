// expect-error: cannot read field `length`
let shared: string | number = "a";
function mutate(): void { shared = 5; }
function main(): number {
  shared = "abc";
  mutate();
  return shared.length;
}
