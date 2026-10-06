import * as embedding from "submilli:embedding";

function main(): void {
  // `* as ns` is a synonym for the default namespace import.
  const r = embedding.embed("fixture-embedding", ["a"], "document");
  assert(r.count === 1, "* as ns synonym binds the package as a namespace");
  assert(embedding.models().length > 0, "and reaches discovery too");
}
