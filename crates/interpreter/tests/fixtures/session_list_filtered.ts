// deny-capability: session.read
import session from "submilli:session";

function main(): void {
  // Writing is a different capability and stays allowed, so the store has
  // entries to hide.
  session.set("triage/a", 1);
  session.set("triage/b", 2);
  session.set("notes/x", 3);

  // `list` gates on `session.list` for the operation and then on `session.read`
  // per candidate key. With every read denied, no key survives the filter — and
  // neither the count nor the cursor may disclose the ones that were dropped.
  const page = session.list("", 100, null);
  assert(page.entries.length === 0, "denied keys are omitted from the page");
  assert(page.nextCursor === null, "the cursor does not disclose denied keys");

  // The filter is silent, not an error: listing a denied keyspace succeeds and
  // reports nothing, so a program cannot probe for existence through a throw.
  const prefixed = session.list("triage/", 10, null);
  assert(prefixed.entries.length === 0, "a denied prefix lists as empty");
  assert(prefixed.nextCursor === null, "no cursor for a fully denied prefix");

  // The op-level gate is separate: `session.list` itself was not denied, so the
  // call runs rather than throwing.
  let threw = false;
  try {
    session.list("triage/", 1, null);
  } catch (e: Error) {
    threw = true;
  }
  assert(!threw, "session.list is gated separately from session.read");

  // A direct read is still refused — the per-key filter and the direct gate are
  // the same capability.
  let denied = "";
  try {
    session.get("triage/a");
  } catch (e: PermissionDeniedError) {
    denied = e.capability;
  }
  assert(denied === "session.read", "get is still gated on session.read");
}
