// deny-capability: llm.call
// A denied `llm.call` is a per-capability refusal, not a blanket kill: the
// program keeps running and every other capability it holds still works. That
// distinction is what makes a deny-by-default policy usable — an operator can
// withhold model access from a program that otherwise needs the session store
// and the filesystem, and the program can detect the refusal and take another
// path rather than dying.
//
// The denial is also gate-first: it is decided before the provider is consulted,
// so a denied caller learns nothing about which models the operator configured.
// The error carries the capability and the caller as structured fields, so
// recovery code branches on those rather than parsing a message.
import llm from "submilli:llm";
import session from "submilli:session";

function main(): void {
  // The denial arrives as a PermissionDeniedError carrying structured fields.
  let capability = "";
  let caller = "";
  let reason = "";
  try {
    llm.call("claude-haiku-4-5", "Summarize this.");
    assert(false, "a denied model call must not dispatch");
  } catch (e: PermissionDeniedError) {
    capability = e.capability;
    caller = e.caller;
    reason = e.reason;
  }
  assert(capability === "llm.call", "the refusal names the capability that was withheld");
  assert(caller === "main", "the refusal attributes to the running caller");
  assert(reason === "denied by fixture policy", "the policy's own reason reaches the guest");

  // R13: a denial message must not become a disclosure channel for the prompt.
  assert(reason.indexOf("Summarize this.") < 0, "the denial never echoes the prompt");

  // `batch` and `models()` are the same capability, so all three are withheld
  // together — one capability, triple-gated, rather than three names.
  let batchDenied = false;
  try {
    llm.batch("claude-haiku-4-5", ["a"]);
  } catch (e: PermissionDeniedError) {
    batchDenied = true;
  }
  assert(batchDenied, "batch is gated on the same capability as call");

  let modelsDenied = false;
  try {
    llm.models();
  } catch (e: PermissionDeniedError) {
    modelsDenied = true;
  }
  assert(modelsDenied, "models() is gated on the same capability as call");

  // The blanket-kill check: a capability the policy did not deny still works.
  // If denial were a process-level kill this would be unreachable.
  session.set("triage/progress", { step: 1 });
  assert(session.has("triage/progress"), "an undenied capability still runs after a denial");
  assert(session.remove("triage/progress"), "and keeps working for the rest of the execution");
}
