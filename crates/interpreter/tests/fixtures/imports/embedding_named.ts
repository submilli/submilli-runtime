import { embed, models } from "submilli:embedding";

function main(): void {
  const r = embed("fixture-embedding", ["a", "b"], "document");
  assert(r.count === 2, "named import dispatches an embed");
  assert(r.vector(0).length === 8, "and reads a vector");
  assert(models().length > 0, "named import reaches discovery");
}
