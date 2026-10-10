// An input over the alias's `maxInputBytes` is refused before anything is sent,
// and the error names the input's index (numbered from 0, like the vectors).
//
// Covers AE1 and the 2,032-byte Google bound: 2,033 bytes on
// gemini-embedding-001 is refused before dispatch; exactly 2,032 is sent.
import embedding from "submilli:embedding";

function main(): void {
  // AE1: the third text is over the 1,536-byte limit of an ordinary alias.
  const long = "z".repeat(1537);
  let caught = "";
  try {
    embedding.embed("fixture-embedding", ["short", "short", long], "document");
    assert(false, "an over-length input must not dispatch");
  } catch (e: RangeError) {
    caught = e.message;
  }
  assert(caught.indexOf("text 2") >= 0, "the error names input index 2");
  assert(caught.indexOf("1536") >= 0, "and the limit it exceeded");
  assert(caught.indexOf("zzzz") < 0, "the error never quotes the input");

  // The limit is inclusive: exactly 1,536 bytes is sent.
  const edge = embedding.embed("fixture-embedding", ["z".repeat(1536)], "document");
  assert(edge.count === 1, "an input exactly at the limit is sent");

  // Google's bound is tighter than 3 x tokens.
  let google = "";
  try {
    embedding.embed("gemini-embedding-001", ["q".repeat(2033)], "query");
    assert(false, "2,033 bytes on gemini-001 must be refused");
  } catch (e: RangeError) {
    google = e.message;
  }
  assert(google.indexOf("text 0") >= 0, "the refused input is index 0");
  assert(google.indexOf("2032") >= 0, "and the gemini-001 bound is named");
  const sent = embedding.embed("gemini-embedding-001", ["q".repeat(2032)], "query");
  assert(sent.count === 1, "exactly 2,032 bytes is sent");

  // Multi-byte text counts bytes, not characters: 600 x 3-byte characters is
  // 1,800 bytes, over an ordinary alias's limit.
  let bytes = false;
  try {
    embedding.embed("fixture-embedding", ["€".repeat(600)], "document");
  } catch (e: RangeError) {
    bytes = true;
  }
  assert(bytes, "the limit is in UTF-8 bytes");
}
