// deny-capability: session.write
import { record } from "@test/store";

function main(): void {
  // The gate names the package whose code is running, not the script that
  // called into it.
  let caller = "";
  let capability = "";
  try {
    record("triage/progress", 1);
  } catch (e: PermissionDeniedError) {
    caller = e.caller;
    capability = e.capability;
  }
  assert(caller === "@test/store", "the gate attributes to the running package");
  assert(capability === "session.write", "set is gated on session.write");
}
