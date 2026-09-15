// A stored value that does not match `T` throws a catchable `TypeError`
// instead of arriving statically typed and unverified. The message names the
// expected type and the runtime kind it found, and never echoes the stored
// value — a session holds a tenant's data, so a type error must not become a
// disclosure channel.
import session from "submilli:session";

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
  // A missing field fails the check.
  session.set("m/partial", { step: 1, note: "no done flag" });
  let threw = false;
  try {
    const p = session.get<Progress>("m/partial");
    assert(false, "a missing field must not pass the check");
  } catch (e: Error) {
    threw = true;
    assert(e.message.indexOf("type mismatch") >= 0, "the failure is a type mismatch");
    assert(e.message.indexOf("no done flag") < 0, "the message never echoes the stored value");
  }
  assert(threw, "a missing field throws");

  // A wrongly-typed field fails the check.
  session.set("m/wrong", { step: "one", note: "n", done: false });
  let wrongField = false;
  try {
    session.get<Progress>("m/wrong");
    assert(false, "a wrongly-typed field must not pass");
  } catch (e: Error) {
    wrongField = true;
    assert(e.message.indexOf("one") < 0, "the message never echoes the offending value");
  }
  assert(wrongField, "a wrongly-typed field throws");

  // A nested mismatch is caught by the same deep walk.
  session.set("m/nested", { id: "a", inner: { step: 1, note: "n" } });
  let nested = false;
  try {
    session.get<Nested>("m/nested");
    assert(false, "a nested mismatch must not pass");
  } catch (e: Error) {
    nested = true;
  }
  assert(nested, "a nested mismatch throws");

  // A whole-value kind mismatch throws too.
  session.set("m/scalar", 3);
  let scalar = false;
  try {
    session.get<Progress>("m/scalar");
    assert(false, "a scalar stored under an object type must not pass");
  } catch (e: Error) {
    scalar = true;
  }
  assert(scalar, "a scalar under an object type throws");

  // A missing key under a non-nullable `T` is a null that fails the check,
  // which is why a possibly-absent key wants `T | null`.
  let absent = false;
  try {
    session.get<Progress>("m/absent");
    assert(false, "a missing key must not pass a non-nullable T");
  } catch (e: Error) {
    absent = true;
  }
  assert(absent, "a missing key throws under a non-nullable T");

  // A union arm that matches nothing stored still throws.
  session.set("m/bool", true);
  let union = false;
  try {
    session.get<string | number>("m/bool");
    assert(false, "a value matching no arm must not pass");
  } catch (e: Error) {
    union = true;
  }
  assert(union, "a value matching no union arm throws");

  // The store is untouched by a failed read.
  assert(session.has("m/partial"), "a failed read leaves the entry in place");
  const recovered = session.get<Progress | null>("m/absent");
  assert(recovered === null, "the session is still usable after a caught mismatch");
}
