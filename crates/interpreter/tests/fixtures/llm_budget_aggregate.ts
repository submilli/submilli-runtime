// The server-wide ceiling is a *different* refusal from the per-execution one,
// and says so. The tokens standing in the way belong to other live executions,
// so the advice that fits the per-execution ceiling — make fewer or shorter
// calls — need not help at all here. Telling a program author to shrink work
// that was never what filled the budget sends them to optimize the wrong thing,
// which is why the two messages are deliberately distinct and why this fixture
// asserts on the difference rather than merely on "a RangeError was thrown".
//
// The fix here belongs to the operator, and the message names the flag.
import llm from "submilli:llm";

function main(): void {
  // The aggregate ceiling here is tighter than this execution's own, so the
  // server-wide limit is what refuses first.
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

  // The ceiling really fired, after admitting what fit.
  assert(caught.length > 0, "the server-wide ceiling eventually refuses a call");
  assert(completed > 0, "but only after admitting the calls that fit");

  // It is the *server* budget that refused, not this execution's.
  assert(caught.indexOf("server token budget") >= 0, "the refusal names the server-wide ceiling");

  // And it says plainly that shrinking this execution's work need not help —
  // the distinction that makes the two ceilings different advice.
  assert(
    caught.indexOf("this execution's own spend is not what is in the way") >= 0,
    "the refusal explains that this execution's spend is not the obstacle",
  );

  // The fix is the operator's, named by flag so it is actionable.
  assert(caught.indexOf("--max-llm-tokens") >= 0, "the refusal names the operator's flag");

  // This is not the per-execution message, which would give the opposite
  // advice. Asserting the absence keeps the two taxonomies from collapsing.
  assert(
    caught.indexOf("execution token budget") < 0,
    "the server-wide refusal is not the per-execution one",
  );

  // R13: a budget refusal carries numbers, never the prompt.
  assert(caught.indexOf("CONFIDENTIAL-PROMPT-TEXT") < 0, "the refusal never echoes the prompt");
}
