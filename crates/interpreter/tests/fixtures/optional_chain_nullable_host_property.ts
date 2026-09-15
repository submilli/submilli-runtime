// A host-backed interface property read inside an optional chain keeps the
// getter's own return type. `Url#port` is `number | null` — nullable, not
// optional — so the wrapper hands back a boxed value, and re-deriving the slot
// from a null-stripped result type would ask for an unboxed `f64` the getter
// never produces (an invalid module, not a trap).

import url from "submilli:url";

interface Wrap {
  u: url.URL;
}

function pick(s: string): url.URL | null {
  if (s === "") {
    return null;
  }
  return url.parse(s);
}

interface Shapes {
  m: Map<string, number>;
  a: string[];
  t: string;
  r: RegExp;
}

interface Inner {
  u: url.URL | null;
}

interface Outer {
  inner: Inner | null;
}

function main(): void {
  const full = url.parse("https://api.test:8443/v1/x?a=1#frag");
  const bare = url.parse("http://plain.test/");
  const w: Wrap | null = { u: full };
  const b: Wrap | null = { u: bare };
  const none: Wrap | null = null;

  // Nullable property, present.
  assert(w?.u.port === 8443, "nullable number property through a chain");
  assert(w?.u.fragment === "frag", "nullable string property through a chain");

  // Nullable property, absent on the URL itself — the getter's own null, not
  // the chain's.
  assert(b?.u.port === null, "getter null reaches the chain result");
  assert(b?.u.fragment === null, "getter null for a nullable string property");

  // Chain short-circuit still wins over the property's own nullability.
  assert(none?.u.port === null, "short-circuit on a null base");

  // Non-nullable properties are unchanged.
  assert(w?.u.host === "api.test", "string property through a chain");
  assert(w?.u.protocol === "https", "string property with a default");
  assert(w?.u.path === "/v1/x", "path property through a chain");

  // The nullable result flows on into `??` and further steps.
  assert((w?.u.port ?? -1) === 8443, "nullable chain result into `??`");
  assert((b?.u.port ?? -1) === -1, "null chain result into `??`");
  assert(w?.u.query.size === 1, "a chain step past the host property");

  // `?.` written on the host-property step itself.
  assert(w?.u?.port === 8443, "optional marker on the host-property step");
  assert(b?.u?.fragment === null, "optional marker with a null-valued getter");

  // The host property as the *first* chain part, on a nullable host base.
  const direct = pick("https://a.test:99/p?x=1#f");
  const bareDirect = pick("http://b.test/");
  const noneDirect = pick("");
  assert(direct?.port === 99, "a nullable number property as the first chain part");
  assert(direct?.fragment === "f", "a nullable string property as the first chain part");
  assert(direct?.host === "a.test", "a non-nullable property as the first chain part");
  assert(bareDirect?.port === null, "the getter's own null as the first chain part");
  assert(noneDirect?.port === null, "short-circuit on a null host base");
  assert((noneDirect?.port ?? -1) === -1, "short-circuit into `??`");
  assert(direct?.query.get("x") === "1", "a method call past the host property");
  assert(JSON.stringify(noneDirect?.port) === "null", "a short-circuited host property stringified");

  // An optional step *on* the nullable host property's own result.
  assert(direct?.fragment?.length === 1, "an optional step after a nullable host property");
  assert(bareDirect?.fragment?.length === null, "the getter's null short-circuits the next step");
  assert(direct?.fragment?.toUpperCase() === "F", "a method call after a nullable host property");

  // Prelude host types through a chain, including the intrinsic `.length`.
  const m = new Map<string, number>();
  m.set("k", 1);
  const sh: Shapes | null = { m: m, a: ["x", "y"], t: "hello", r: new RegExp("a(b)c", "g") };
  const noSh: Shapes | null = null;
  assert(sh?.m.size === 1, "`Map#size` through a chain");
  assert(sh?.a.length === 2, "the intrinsic `.length` branch through a chain");
  assert(sh?.t.length === 5, "and on a string");
  assert(noSh?.a.length === null, "a short-circuited intrinsic `.length`");
  assert(JSON.stringify(noSh?.a.length) === "null", "and it stringifies as null");
  assert(sh?.a[0].length === 1, "`.length` after an index step");
  assert(sh?.r.source === "a(b)c", "a `RegExp` property through a chain");
  assert(sh?.r.global === true, "a boolean `RegExp` property through a chain");
  assert(sh?.m.get("k") === 1, "a chain ending in a method call");
  assert(sh!.m.size === 1, "a `!` in place of the last `?.`");

  // Three optional steps down to a nullable host property.
  const o: Outer | null = { inner: { u: url.parse("https://a.test:99/p") } };
  const mid: Outer | null = { inner: null };
  assert(o?.inner?.u?.port === 99, "three optional steps to a nullable host property");
  assert(mid?.inner?.u?.port === null, "a null at the middle step");
  assert((mid?.inner?.u?.port ?? -1) === -1, "a middle null into `??`");

  // Outside a chain, for parity.
  assert(full.port === 8443, "non-chain read of the same nullable property");
  assert(bare.port === null, "non-chain read of an absent nullable property");
}
