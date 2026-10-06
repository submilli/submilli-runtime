// Embed a batch and read the sealed result. The vectors stay in the host: the
// program reads one at a time as numbers (`vector`) or as the compact bytes
// (`bytes`, little-endian f32), and the scalar facts are plain properties.
//
// Covers R1, R2, R24 and AE8 (the range check), and AE9: a `query` and a
// `document` call on one alias carry the same identity.
import embedding from "submilli:embedding";

// Decode the little-endian f32 at element `i` of `b` by hand, so the check
// does not trust the host's own encoding of what it is verifying.
function f32At(b: Uint8Array, i: number): number {
  const o = i * 4;
  const bits = b[o] + b[o + 1] * 256 + b[o + 2] * 65536 + b[o + 3] * 16777216;
  const sign = bits >= 2147483648 ? -1 : 1;
  const rest = bits % 2147483648;
  const exp = Math.floor(rest / 8388608);
  const man = rest % 8388608;
  if (exp === 0) {
    return sign * man * Math.pow(2, -149);
  }
  return sign * (1 + man / 8388608) * Math.pow(2, exp - 127);
}

function main(): void {
  const r = embedding.embed("fixture-embedding", ["alpha", "beta", "gamma"], "document");
  assert(r.count === 3, "one vector per input");
  assert(r.dimensions === 8, "the dimensions the alias declares");
  assert(r.identity.length > 0, "the result is labeled with an identity");
  assert(r.model === "fixture-embedding", "and with the alias it was embedded with");
  assert(r.inputTokens !== null, "the provider reported usage");

  const v0 = r.vector(0);
  assert(v0.length === r.dimensions, "vector(0) has `dimensions` numbers");
  let norm = 0;
  for (let i = 0; i < v0.length; i = i + 1) {
    norm = norm + v0[i] * v0[i];
  }
  assert(Math.abs(norm - 1) < 1e-5, "the vector is unit length");

  // bytes(i) is the same vector as little-endian f32: dimensions * 4 bytes.
  const b1 = r.bytes(1);
  assert(b1.length === r.dimensions * 4, "bytes(1) is dimensions * 4 bytes");
  const v1 = r.vector(1);
  for (let i = 0; i < r.dimensions; i = i + 1) {
    assert(Math.abs(f32At(b1, i) - v1[i]) < 1e-6, "the bytes decode to the same values as vector(1)");
  }
  const v2 = r.vector(2);
  let differs = false;
  for (let i = 0; i < r.dimensions; i = i + 1) {
    if (v1[i] !== v2[i]) {
      differs = true;
    }
  }
  assert(differs, "vectors are positional: each input has its own");

  // AE8: reading past the end is the program's mistake, a RangeError.
  let caught = "";
  try {
    r.vector(r.count);
    assert(false, "vector(count) must throw");
  } catch (e: RangeError) {
    caught = e.message;
  }
  assert(caught.indexOf("out of range") >= 0, "vector(count) is a RangeError naming the range");
  assert(caught.indexOf("3") >= 0, "and says how many vectors there are");
  let negative = false;
  try {
    r.bytes(-1);
  } catch (e: RangeError) {
    negative = true;
  }
  assert(negative, "a negative index is a RangeError");
  let fractional = false;
  try {
    r.vector(0.5);
  } catch (e: RangeError) {
    fractional = true;
  }
  assert(fractional, "a fractional index is a RangeError");
  let notANumber = false;
  try {
    r.bytes(0 / 0);
  } catch (e: RangeError) {
    notANumber = true;
  }
  assert(notANumber, "NaN is a RangeError");

  // AE9: purpose is not part of the identity, though the vectors may differ.
  const q = embedding.embed("fixture-embedding", ["alpha"], "query");
  assert(q.identity === r.identity, "query and document results share an identity");
  assert(q.vector(0)[0] !== v0[0] || q.vector(0)[1] !== v0[1], "while the vectors themselves may differ by purpose");
}
