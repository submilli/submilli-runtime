// deny-capability: session.remove
import session from "submilli:session";

function main(): void {
  // Denying removal leaves writing alone.
  session.set("triage/keep", { step: 1 });
  assert(session.has("triage/keep"), "set still runs");

  let denied = "";
  try {
    session.remove("triage/keep");
  } catch (e: PermissionDeniedError) {
    denied = e.capability + ":" + e.caller;
  }
  assert(denied === "session.remove:main", "remove is gated on session.remove");
  assert(session.has("triage/keep"), "a denied remove leaves the entry intact");
}
