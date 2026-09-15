// `instanceof Uint8Array` on bytes the guest never constructed. The test is
// structural (`ref.test $Uint8Array`), so a host function that built its result
// through a differently-shaped struct would read as "not bytes" — a silent
// false negative on the most common way a real program obtains a Uint8Array.

import { sha256, randomBytes } from "submilli:crypto";

function isBytes(x: unknown): boolean {
  return x instanceof Uint8Array;
}

function main(): void {
  const digest: Uint8Array = sha256("hello");
  assert(digest.length === 32, "sha256 returns 32 bytes");
  assert(isBytes(digest), "host-built digest is a Uint8Array");

  const random: Uint8Array = randomBytes(16);
  assert(isBytes(random), "host-built random bytes are a Uint8Array");

  const encoded: Uint8Array = new TextEncoder().encode("hello");
  assert(encoded.length === 5, "encode returns 5 bytes for ASCII");
  assert(isBytes(encoded), "encoder output is a Uint8Array");

  const fromHex: Uint8Array = Uint8Array.fromHex("0b0b");
  assert(isBytes(fromHex), "fromHex output is a Uint8Array");

  const fromBase64: Uint8Array = Uint8Array.fromBase64("aGk=");
  assert(isBytes(fromBase64), "fromBase64 output is a Uint8Array");

  // Guest-constructed bytes are the baseline the host values must match.
  assert(isBytes(new Uint8Array([1, 2, 3])), "guest-built bytes still test true");
}
