// deny-capability: session.write
import session from "submilli:session";

function main(): void {
  // A denied write throws a catchable PermissionDeniedError carrying the
  // capability and the key the policy matched on.
  let denied = "";
  try {
    session.set("triage/progress", { step: 1 });
  } catch (e: PermissionDeniedError) {
    denied = e.capability + ":" + e.caller;
  }
  assert(denied === "session.write:main", "set is gated on session.write");

  // Reads are a different capability and stay allowed.
  assert(session.get("triage/progress") === undefined, "get still runs");
  assert(!session.has("triage/progress"), "has still runs");

  // So is remove, which the policy did not deny.
  assert(!session.remove("triage/progress"), "remove still runs");
}
