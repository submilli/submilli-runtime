function main(): void {
  let value: string | null = "outer";
  assert(value.length === 5, "let initializer");
  value = null;
  assert(value === null, "declared storage permits null");
  value = "again";
  assert(value.length === 5, "assignment narrows again");
  const fixed: number | null = 3;
  assert(fixed + 1 === 4, "const initializer");
  const read = (): number => fixed + 2;
  assert(read() === 5, "captured const");
  let choice: "a" | "b" = "a";
  assert(choice === "a", "literal union");
  choice = "b";
  assert(choice === "b", "literal reassignment");
  { let value: number | null = 7; assert(value + 1 === 8, "shadow"); }
  assert(value.length === 5, "outer survives shadow");
}
