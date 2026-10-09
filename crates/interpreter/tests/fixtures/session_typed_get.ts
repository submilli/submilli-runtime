// `get<T>` checks the stored value against `T` structurally and hands back a
// value that is really a `T`, or undefined when the key is absent. Every import form reaches the same check, since
// the rewrite keys on the package-export mangled name rather than on whatever
// the receiver happens to be called.
import session from "submilli:session";
import kv from "submilli:session";
import { get, set } from "submilli:session";

interface Progress {
  step: number;
  note: string;
  done: boolean;
}

interface Nested {
  id: string;
  inner: Progress;
}

function main(): void {
  session.set("triage/progress", { step: 1, note: "start", done: false });

  const p = session.get<Progress>("triage/progress");
  if (p === undefined) throw new Error("stored value must be present");
  assert(p.step === 1, "step round-trips through the checked read");
  assert(p.note === "start", "note round-trips");
  assert(!p.done, "done round-trips");

  // An aliased namespace import binds the same export, so it validates too.
  const viaAlias = kv.get<Progress>("triage/progress");
  if (viaAlias === undefined) throw new Error("stored value must be present");
  assert(viaAlias.step === 1, "an aliased namespace import still validates");

  // The named-import form is the same symbol and behaves identically.
  const viaNamed = get<Progress>("triage/progress");
  if (viaNamed === undefined) throw new Error("stored value must be present");
  assert(viaNamed.step === 1, "a named import still validates");

  // A nested interface is reduced to its object shape and checked all the way down.
  set("triage/nested", { id: "a", inner: { step: 2, note: "deep", done: true } });
  const n = get<Nested>("triage/nested");
  if (n === undefined) throw new Error("stored value must be present");
  assert(n.id === "a", "outer field round-trips");
  assert(n.inner.step === 2, "nested field round-trips");
  assert(n.inner.done, "nested boolean round-trips");

  // A nullable `T` accepts both a stored value and a stored null, and a
  // missing key reads as undefined under every type.
  const maybe = session.get<Progress | null>("triage/progress");
  assert(maybe !== undefined && maybe !== null && maybe.step === 1, "nullable T accepts a value");

  session.set("triage/empty", null);
  assert(session.get<Progress | null>("triage/empty") === null, "nullable T accepts stored null");
  assert(session.get<Progress | null>("triage/absent") === undefined, "missing keys return undefined");

  // A union type argument validates against each arm in turn.
  session.set("triage/scalar", 7);
  const asNumber = session.get<string | number>("triage/scalar");
  assert(asNumber === 7, "the number arm of a union validates");

  session.set("triage/scalar", "seven");
  const asString = session.get<string | number>("triage/scalar");
  assert(asString === "seven", "the string arm of the same union validates");

  // Arrays and primitives are verifiable targets on their own.
  session.set("triage/list", [1, 2, 3]);
  const list = session.get<number[]>("triage/list");
  if (list === undefined) throw new Error("stored value must be present");
  assert(list.length === 3, "array length survives the check");
  assert(list[2] === 3, "array element survives the check");

  // A bare `get` keeps its pre-generic meaning: an unchecked read of
  // `unknown` the caller narrows with a runtime-checked `as`.
  const untyped = session.get("triage/progress") as Progress;
  assert(untyped.step === 1, "the untyped read still works");

  // A local named `session` is an ordinary variable — the rewrite keys on the
  // resolved export, never on the receiver's spelling, so this cannot collide.
  const shadow = { get: 5 };
  assert(shadow.get === 5, "a user value named like the namespace is untouched");
}
