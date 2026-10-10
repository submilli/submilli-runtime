// Exercises the host-owned $Uint8Array vtable polymorphically: structural
// `equals`/`hash` through a Set, and `toString`/`toJson` through unions — all
// agreeing with host-built construction.

function main(): void {
  // hash + equals via Set membership (two distinct-but-equal instances dedup).
  const seen = new Set<Uint8Array>();
  seen.add(Uint8Array.fromHex("0a0b0c"));
  seen.add(Uint8Array.new([10, 11, 12]));
  seen.add(Uint8Array.of(1, 2));
  assert(seen.size === 2, "equal byte sequences dedup in a Set");
  assert(seen.has(Uint8Array.fromHex("0a0b0c")), "membership via hash + equals");
  assert(!seen.has(Uint8Array.alloc(3)), "distinct bytes are absent");

  // toString is the comma-joined decimals (shared with the vtable slot 0 logic).
  const u: Uint8Array = Uint8Array.new([1, 44, 255]);
  assert(u.toString() === "1,44,255", "toString is comma-joined decimals");

  // url-safe vs standard base64 round-trips through the constructor host fns.
  const bytes = Uint8Array.new([251, 255, 16]);
  const urlSafe = bytes.toBase64({ alphabet: "base64url" });
  const standard = bytes.toBase64();
  assert(urlSafe === "-_8Q", "url-safe alphabet");
  assert(standard === "+/8Q", "standard alphabet");
  assert(
    Uint8Array.fromBase64(urlSafe, { alphabet: "base64url" }).equals(bytes),
    "url-safe round-trips"
  );
  assert(Uint8Array.fromBase64(standard).equals(bytes), "standard round-trips");

  // omitPadding option drops the trailing '='.
  const padded = Uint8Array.new([104]);
  assert(padded.toBase64() === "aA==", "padded by default");
  assert(padded.toBase64({ omitPadding: true }) === "aA", "omitPadding drops '='");
}
