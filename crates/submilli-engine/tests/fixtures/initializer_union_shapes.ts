interface Item { value: number; note?: string; }
function main(): void {
  let item: Item | null = { value: 4 };
  item.note = "present";
  assert(item.note === "present", "retain declared optional field");
  item = null;
  assert(item === null, "nullable storage");
  const frozen: { readonly value: number } | null = { value: 3 };
  assert(frozen.value === 3, "readonly declared member");
  let total: number | null = 0;
  for (let i: number = 0; i < 3; i++) { total = total + i; }
  assert(total === 3, "loop initializer fact");
}
