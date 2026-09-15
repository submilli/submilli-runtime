import session from "submilli:session";

interface Progress {
  step: number;
  note: string;
  done: boolean;
}

function main(): void {
  // A missing key reads as null and reports absent.
  assert(session.get("triage/progress") === null, "missing key reads null");
  assert(!session.has("triage/progress"), "missing key is absent");

  session.set("triage/progress", { step: 1, note: "start", done: false });
  assert(session.has("triage/progress"), "written key is present");

  const stored = session.get("triage/progress") as Progress;
  assert(stored.step === 1, "step round-trips");
  assert(stored.note === "start", "note round-trips");
  assert(!stored.done, "done round-trips");

  // Each read produces an independent value: mutating it must not reach the
  // store, which only another `set` may change.
  stored.step = 99;
  const reread = session.get("triage/progress") as Progress;
  assert(reread.step === 1, "reads are independent of each other");

  session.set("triage/progress", { step: 2, note: "next", done: true });
  const updated = session.get("triage/progress") as Progress;
  assert(updated.step === 2, "set replaces the entry");
  assert(updated.done, "boolean survives replacement");

  // A stored null is a present entry; only `has` separates it from absent.
  session.set("triage/empty", null);
  assert(session.get("triage/empty") === null, "stored null reads as null");
  assert(session.has("triage/empty"), "stored null is still present");

  // `remove` reports whether the entry existed.
  assert(session.remove("triage/empty"), "remove reports the entry existed");
  assert(!session.remove("triage/empty"), "removing twice reports absent");
  assert(!session.has("triage/empty"), "removed key is absent");

  // Arrays, strings, and numbers round-trip on their own.
  session.set("triage/list", [1, 2, 3]);
  const list = session.get("triage/list") as number[];
  assert(list.length === 3, "array length round-trips");
  assert(list[2] === 3, "array element round-trips");

  session.set("triage/text", "plain \"quoted\" \\ text\n");
  assert(session.get("triage/text") as string === "plain \"quoted\" \\ text\n", "string escapes round-trip");

  session.set("triage/count", 42);
  assert(session.get("triage/count") as number === 42, "number round-trips");

  session.set("triage/flag", true);
  assert(session.get("triage/flag") as boolean, "boolean round-trips");

  // An empty key is rejected, and the rejection is catchable.
  let emptyKey = false;
  try {
    session.set("", 1);
  } catch (e: Error) {
    emptyKey = true;
  }
  assert(emptyKey, "an empty key is rejected");
}
