// R14: prompt size and prompt count are bounded *independently of the token
// ceiling*, and independently of each other. A thousand tiny prompts and one
// enormous prompt are both pathological, and neither is caught by the other's
// limit — nor reliably by a token budget, which a caller can stay under while
// still handing the host a slice it should refuse to marshal.
//
// Both bounds are checked before any reservation is taken, so an oversized
// batch is refused without charging the budget and without partially
// dispatching. Both count rather than quote: the refusal reports how many
// elements or how many bytes, never the text.
import llm from "submilli:llm";

function main(): void {
  // Inside both bounds, a batch dispatches normally.
  const ok = llm.batch("claude-haiku-4-5", ["a", "b", "c"]);
  assert(ok.length === 3, "a batch inside both bounds dispatches");

  // Too many prompts in one slice is refused, by count.
  let countCaught = "";
  try {
    llm.batch("claude-haiku-4-5", ["a", "b", "c", "d", "e", "f"]);
    assert(false, "an over-count batch must not dispatch");
  } catch (e: RangeError) {
    countCaught = e.message;
  }
  assert(countCaught.length > 0, "too many prompts is refused as a catchable RangeError");
  assert(countCaught.indexOf("prompts in one batch") >= 0, "the refusal names the count bound");
  assert(countCaught.indexOf("prompt bound exceeded") >= 0, "and identifies itself as a prompt bound");

  // The refusal reports the numbers, so a caller can resize without guessing.
  assert(countCaught.indexOf("6") >= 0, "the refusal reports how many were sent");
  assert(countCaught.indexOf("4") >= 0, "and how many are allowed");

  // One oversized prompt is refused by a different bound, even though the
  // slice is well inside the count limit. This is the independence R14 asks
  // for: the count bound would never have caught this one.
  let bytesCaught = "";
  const huge =
    "CONFIDENTIAL-PROMPT-TEXT-0123456789-0123456789-0123456789-0123456789-0123456789";
  try {
    llm.call("claude-haiku-4-5", huge);
    assert(false, "an oversized prompt must not dispatch");
  } catch (e: RangeError) {
    bytesCaught = e.message;
  }
  assert(bytesCaught.length > 0, "an oversized prompt is refused as a catchable RangeError");
  assert(bytesCaught.indexOf("bytes in a single prompt") >= 0, "the refusal names the size bound");

  // And it is *not* the count bound's message — the two report different fixes.
  assert(bytesCaught.indexOf("prompts in one batch") < 0, "the size bound is not the count bound");

  // R13: a bounds refusal counts the prompt, it never quotes it.
  assert(bytesCaught.indexOf("CONFIDENTIAL-PROMPT-TEXT") < 0, "the refusal never echoes the prompt");

  // Neither refusal is a budget refusal: the bounds are checked before any
  // reservation, so nothing was charged and the token ceiling is untouched.
  assert(countCaught.indexOf("token budget") < 0, "a count refusal is not a budget refusal");
  assert(bytesCaught.indexOf("token budget") < 0, "a size refusal is not a budget refusal");

  // The budget really is intact — a normal call still dispatches afterwards.
  const after = llm.call("claude-haiku-4-5", "small");
  assert(after.ok, "a refused-for-bounds call charged nothing against the budget");
}
