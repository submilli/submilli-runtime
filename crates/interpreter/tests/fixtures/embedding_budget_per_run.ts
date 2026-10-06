// An embedding call that would exceed the run's embedding token budget is
// refused before anything is sent, as a `QuotaExceededError` the program can
// catch. The fixture's ceiling is 100 tokens; a 180-byte text estimates 60.
//
// Covers AE6. (The provider-not-called and budget-unchanged halves are pinned
// by the Rust tests in stdlib/embedding, which can see the provider.)
import embedding from "submilli:embedding";

function main(): void {
  const text = "w".repeat(180);

  const first = embedding.embed("fixture-embedding", [text], "document");
  assert(first.count === 1, "a call within the budget dispatches");
  assert(first.inputTokens === 60, "the usage the provider reported reaches the program");

  let caught = "";
  try {
    embedding.embed("fixture-embedding", [text], "document");
    assert(false, "the second call exceeds the 100-token ceiling");
  } catch (e: QuotaExceededError) {
    assert(e instanceof Error, "quota is an Error");
    assert(!((e as unknown) instanceof RangeError), "quota is not an argument error");
    caught = e.message;
  }
  assert(caught.indexOf("execution embedding token budget") >= 0, "the refusal names the execution budget");
  assert(caught.indexOf("wwww") < 0, "and never quotes the input");

  // A smaller call that still fits is admitted: the refusal charged nothing.
  const small = embedding.embed("fixture-embedding", ["w".repeat(90)], "document");
  assert(small.count === 1, "the refused call left the budget intact");
}
