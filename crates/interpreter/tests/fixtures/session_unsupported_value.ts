import session from "submilli:session";

interface Kept {
  step: number;
}

function main(): void {
  session.set("triage/kept", { step: 1 });

  // A function has no JSON form. Rejecting it must leave the previous entry
  // alone rather than replace it with a stringified stand-in.
  let rejected = false;
  try {
    session.set("triage/kept", (n: number): number => n + 1);
  } catch (e: Error) {
    rejected = true;
  }
  assert(rejected, "a function value is rejected");
  assert((session.get("triage/kept") as Kept).step === 1, "the previous entry survives");

  // Same for a value reachable from itself.
  const cycle: unknown[] = [];
  cycle.push(cycle);
  let cyclic = false;
  try {
    session.set("triage/kept", cycle);
  } catch (e: Error) {
    cyclic = true;
  }
  assert(cyclic, "a cyclic value is rejected");
  assert((session.get("triage/kept") as Kept).step === 1, "the previous entry still survives");

  // And for a Map, which the language models as a collection rather than data.
  const m = new Map<string, number>();
  m.set("a", 1);
  let collection = false;
  try {
    session.set("triage/kept", m);
  } catch (e: Error) {
    collection = true;
  }
  assert(collection, "a Map is rejected");
  assert((session.get("triage/kept") as Kept).step === 1, "the previous entry survives a Map");
}
