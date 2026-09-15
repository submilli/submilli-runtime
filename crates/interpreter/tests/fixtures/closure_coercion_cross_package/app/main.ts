import { adapt, roundtrip, drain, Sink } from "@test/callbacks";
function main(): void {
  const original = (): never => { throw new Error("boom"); };
  const fromLibrary = adapt(original);
  const local: () => void = original;
  assert(local === fromLibrary, "identity across module boundaries");
  assert(fromLibrary === local, "identity comparison is symmetric");
  assert(roundtrip<void>(local) === local, "identity across nested adapters");
  let calls = 0;
  const sink: Sink<void> = { emit: (): void => { calls++; } };
  drain<void>(sink);
  assert(calls === 1, "generic interface across package boundary");
  try { fromLibrary(); } catch (e) { calls++; }
  assert(calls === 2, "original exception crosses adapter");
}
