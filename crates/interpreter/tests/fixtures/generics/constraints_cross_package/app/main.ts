import { getId, Box, Holder, Wrapped } from "@test/constrained";
function main(): void {
  const holder: Holder<{ id: number }> = { value: { id: 42 } };
  const wrapped: Wrapped<{ id: number }> = holder;
  assert(getId(wrapped.value) === 42, "imported function constraint");
  assert(new Box(wrapped.value).id() === 42, "imported class constraint");
}
