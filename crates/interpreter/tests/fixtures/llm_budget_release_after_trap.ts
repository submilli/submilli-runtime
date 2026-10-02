// A dispatch that never ran returns its whole reservation immediately, rather
// than holding it until teardown. This is the defect the budget was written
// around: tokens are reserved *before* the provider is consulted, so a refused
// or failed dispatch that kept its reservation would ratchet the budget toward
// its cap with no recovery, and a long-running execution would starve itself on
// calls that cost nothing.
//
// The observable consequence is the one a program actually depends on: after
// catching a failed call, the very next call still fits. If the reservation
// leaked, this fixture's later calls would be refused for budget reasons even
// though nothing was ever billed.
import llm from "submilli:llm";

function main(): void {
  // A baseline call succeeds and reconciles to its reported usage.
  const first = llm.call("claude-haiku-4-5", "CONFIDENTIAL-PROMPT-TEXT");
  assert(first.ok, "the baseline call dispatches");

  // Now fail a dispatch repeatedly against an undeclared model. Each one is
  // rejected by the provider, so nothing is billed — and each reservation must
  // come back. Enough iterations that a leak would exhaust the ceiling.
  let rejected = 0;
  for (let i = 0; i < 12; i = i + 1) {
    try {
      llm.call("no-such-model", "CONFIDENTIAL-PROMPT-TEXT");
      assert(false, "an undeclared model must not dispatch");
    } catch (e: Error) {
      rejected = rejected + 1;
      // The failure is the model rejection, not a budget refusal — if the
      // reservations were leaking, this would turn into a QuotaExceededError about
      // the token ceiling instead, and the assertion below would catch it.
      assert(
        e.message.indexOf("token budget") < 0,
        "a failed dispatch is refused for its model, never for a budget it returned",
      );
      assert(e.message.indexOf("CONFIDENTIAL-PROMPT-TEXT") < 0, "and never echoes the prompt");
    }
  }
  assert(rejected === 12, "every undeclared-model call was rejected");

  // The budget survived all of it: a normal call still fits, which it could
  // not if twelve reservations had leaked.
  const after = llm.call("claude-haiku-4-5", "CONFIDENTIAL-PROMPT-TEXT");
  assert(after.ok, "a call still fits after a dozen failed dispatches returned their reservations");
  assert(after.inputTokens === 10, "and reconciles against real reported usage");

  // A batch whose prompts exceed nothing still works afterwards too — the
  // release path is not specific to single calls.
  const batched = llm.batch("claude-haiku-4-5", ["one", "two"]);
  assert(batched.length === 2, "a batch still fits after the failed dispatches");
  assert(batched[0].ok, "and its elements complete normally");
}
