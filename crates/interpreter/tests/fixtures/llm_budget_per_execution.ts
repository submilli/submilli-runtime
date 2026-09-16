// An execution that would exceed its own token ceiling is refused *before*
// dispatch, not after the provider has billed. The reservation covers output
// tokens as well as input, so the ceiling is preventive rather than
// retroactive — a program cannot spend past its budget and discover it
// afterwards.
//
// The refusal is a `RangeError` rather than an opaque trap, because a quota is
// something a program can catch and adapt to: shorten the work, batch less, or
// report that it ran out. The message names this execution's own ceiling and
// suggests the fix that actually applies here — fewer or shorter calls — which
// is deliberately *not* the advice the server-wide ceiling gives.
import llm from "submilli:llm";

function main(): void {
  // Calls inside the ceiling succeed and are billed against it.
  const first = llm.call("claude-haiku-4-5", "CONFIDENTIAL-PROMPT-TEXT");
  assert(first.ok, "a call within the budget dispatches");
  assert(first.inputTokens === 10, "the provider's reported usage reaches the guest");
  assert(first.outputTokens === 10, "for both directions");

  // Keep calling until the ceiling refuses one. Each call reserves its prompt
  // estimate plus the output cap, so the budget is consumed in finite steps
  // and this loop terminates well before its bound.
  let caught = "";
  let completed = 0;
  for (let i = 0; i < 20; i = i + 1) {
    try {
      llm.call("claude-haiku-4-5", "CONFIDENTIAL-PROMPT-TEXT");
      completed = completed + 1;
    } catch (e: RangeError) {
      caught = e.message;
      break;
    }
  }

  // The ceiling really was reached — a fixture where it never fired would pass
  // every assertion below vacuously.
  assert(caught.length > 0, "the per-execution ceiling eventually refuses a call");
  assert(completed > 0, "but only after admitting the calls that fit");

  // The refusal is catchable as a RangeError, which is what lets a program
  // treat running out of budget as a condition rather than a crash.
  assert(caught.indexOf("execution token budget") >= 0, "the refusal names the execution ceiling");

  // It suggests the fix that applies to *this* ceiling: this execution's own
  // spend is what filled it, so spending less genuinely helps.
  assert(caught.indexOf("fewer or shorter calls") >= 0, "and names the fix the caller can act on");

  // R13: a budget refusal carries numbers, never the prompt that triggered it.
  assert(caught.indexOf("CONFIDENTIAL-PROMPT-TEXT") < 0, "the refusal never echoes the prompt");

  // Execution continues past the refusal — the budget stops model calls, not
  // the program.
  assert(true, "a caught budget refusal does not end the execution");
}
