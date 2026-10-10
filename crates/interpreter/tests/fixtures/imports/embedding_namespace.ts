import embedding from "submilli:embedding";

function main(): void {
  const r = embedding.embed("fixture-embedding", ["a"], "query");
  assert(r.count === 1, "namespace import dispatches an embed");
  assert(embedding.models().length > 0, "and reaches discovery through the same binding");
}
