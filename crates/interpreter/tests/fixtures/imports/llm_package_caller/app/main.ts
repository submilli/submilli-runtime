// deny-capability: llm.call
import { classify } from "@test/triage";

function main(): void {
  // The gate names the package whose code is running, not the script that
  // called into it. This is what makes a per-package grant meaningful: an
  // operator allowing `llm.call` for a triage library is allowing that
  // library's own calls, and a script cannot borrow the library's identity to
  // widen its own reach.
  let caller = "";
  let capability = "";
  try {
    classify("claude-haiku-4-5", "CONFIDENTIAL-PROMPT-TEXT");
  } catch (e: PermissionDeniedError) {
    caller = e.caller;
    capability = e.capability;
  }
  assert(caller === "@test/triage", "the gate attributes to the running package");
  assert(capability === "llm.call", "a model call is gated on llm.call");
}
