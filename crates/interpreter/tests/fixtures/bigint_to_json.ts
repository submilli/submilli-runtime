// BigInt#toJson and JSON serialization route through the host-owned vtable
// toJson slot / the ported prelude-host method. JSON has no native bigint
// literal, so a bigint renders as its canonical decimal (unquoted, == toString).

interface Holder {
  count: bigint;
  label: string;
}

function main(): void {
  // Direct method: matches the decimal toString.
  assert((42n).toJson() === "42", "scalar toJson");
  assert((-7n).toJson() === "-7", "negative toJson carries sign");
  assert(123456789012345678901234567890n.toJson() === "123456789012345678901234567890", "huge toJson");

  // Object field: the object vtable toJson dispatches into the bigint slot; the
  // decimal is inserted unquoted, the string field is quoted.
  const h: Holder = { count: 256n, label: "ok" };
  assert(JSON.stringify(h) === `{"count":256,"label":"ok"}`, "object with bigint field");

  // Array of bigints: each element serializes as a bare decimal.
  const xs: bigint[] = [1n, -2n, 3n];
  assert(JSON.stringify(xs) === "[1,-2,3]", "bigint array json");
}
