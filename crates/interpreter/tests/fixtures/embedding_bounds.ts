// Per-call caps: at most 128 texts and 2 MiB of text in one call, each refused
// as a `RangeError` before the provider or the budget is involved.
// Covers R18.
import embedding from "submilli:embedding";

function main(): void {
  const many: string[] = [];
  for (let i = 0; i < 129; i = i + 1) {
    many.push("t");
  }
  let count = "";
  try {
    embedding.embed("fixture-embedding", many, "document");
    assert(false, "129 texts must be refused");
  } catch (e: RangeError) {
    count = e.message;
  }
  assert(count.indexOf("texts in one call") >= 0, "the count bound is named");
  assert(count.indexOf("129") >= 0, "with how many were sent");
  assert(count.indexOf("128") >= 0, "and how many are allowed");

  // 128 texts is exactly the cap and is accepted.
  const exact = many.slice(0, 128);
  assert(embedding.embed("fixture-embedding", exact, "document").count === 128, "128 texts is accepted");

  // Three 1,000,000-byte texts are 3,000,000 bytes, over 2 MiB (2,097,152).
  const big = "b".repeat(1000000);
  let bytes = "";
  try {
    embedding.embed("fixture-embedding", [big, big, big], "document");
    assert(false, "over 2 MiB must be refused");
  } catch (e: RangeError) {
    bytes = e.message;
  }
  assert(bytes.indexOf("bytes in one call") >= 0, "the byte bound is named");
  assert(bytes.indexOf("2097152") >= 0, "with the limit");

  // An empty call is the program's mistake too.
  let empty = false;
  try {
    embedding.embed("fixture-embedding", [], "document");
  } catch (e: RangeError) {
    empty = true;
  }
  assert(empty, "an empty batch is a RangeError");
}
