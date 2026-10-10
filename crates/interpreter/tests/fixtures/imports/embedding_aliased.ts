import { embed as vectorize, models as catalog } from "submilli:embedding";

function main(): void {
  // The gate keys on the resolved export, never on the name the caller chose.
  assert(vectorize("fixture-embedding", ["a"], "document").count === 1, "aliased import dispatches an embed");
  assert(catalog().length > 0, "aliased import reaches discovery");
}
