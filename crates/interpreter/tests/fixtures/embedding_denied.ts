// deny-capability: embedding.embed
// A denied `embedding.embed` is a per-capability refusal: the program keeps
// running, the denial is attributed to the caller, and it is decided before the
// provider is consulted, so a denied caller learns nothing about the aliases
// the operator configured.
import embedding from "submilli:embedding";
import session from "submilli:session";

function main(): void {
  let capability = "";
  let caller = "";
  let reason = "";
  try {
    embedding.embed("fixture-embedding", ["Summarize this."], "document");
    assert(false, "a denied embed must not dispatch");
  } catch (e: PermissionDeniedError) {
    capability = e.capability;
    caller = e.caller;
    reason = e.reason;
  }
  assert(capability === "embedding.embed", "the refusal names the withheld capability");
  assert(caller === "main", "and attributes to the running caller");
  assert(reason === "denied by fixture policy", "the policy's own reason reaches the guest");
  assert(reason.indexOf("Summarize this.") < 0, "the denial never echoes the input");

  // Even an alias that does not exist is denied, not reported unknown: the
  // gate runs before alias resolution.
  let ghost = false;
  try {
    embedding.embed("no-such-alias", ["x"], "document");
  } catch (e: PermissionDeniedError) {
    ghost = true;
  }
  assert(ghost, "a denied caller cannot probe which aliases exist");

  assert(embedding.models().length === 0, "discovery reveals nothing to a denied caller");

  session.set("k", { step: 1 });
  assert(session.has("k"), "an undenied capability still runs after a denial");
}
