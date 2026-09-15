// Divergence pin: ECMA-262 byte stores go through ToUint8 (modulo 2^8), so a
// JS write of -1 lands as 255. Here writes truncate non-negative values to
// the low 8 bits (256 → 0, like JS) but saturate negative values to 0
// (spec.md §1.2) — same contract as the constructor.

function main(): void {
  const bytes: Uint8Array = Uint8Array.alloc(1);

  bytes[0] = 256;
  assertSameValue(bytes[0], 0, "256 wraps to 0 (matches JS ToUint8)");
  bytes[0] = 511;
  assertSameValue(bytes[0], 255, "511 wraps to 255 (matches JS ToUint8)");

  bytes[0] = -1;
  assertSameValue(bytes[0], 0, "-1 saturates to 0 (JS would store 255)");
  bytes[0] = -255;
  assertSameValue(bytes[0], 0, "-255 saturates to 0 (JS would store 1)");

  const ctor: Uint8Array = new Uint8Array([-1, 256, 511]);
  assertSameValue(ctor[0], 0, "constructor saturates -1 to 0");
  assertSameValue(ctor[1], 0, "constructor wraps 256 to 0");
  assertSameValue(ctor[2], 255, "constructor wraps 511 to 255");
}
