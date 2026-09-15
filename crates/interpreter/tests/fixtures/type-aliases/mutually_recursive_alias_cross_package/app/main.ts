import { nullOf, someOf, take } from "@test/ring";

function main(): void {
  assert(nullOf() === null, "a library-returned recursive alias whose name was never imported");
  assert((nullOf() ?? 7) === 7, "`??` on it takes the right side — no bogus warning");
  assert((someOf() ?? 7) === 4, "and keeps a non-null arm");
  assert(JSON.stringify(nullOf()) === "null", "and it serializes");
  assert(take(null) === "null", "passing null into a library fn over the alias");
}
