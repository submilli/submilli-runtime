import { alias, assertIsString, assertPresent } from "@test/checks";
import * as checks from "@test/checks";

function main(): void {
  const v: unknown = "hi";
  assertIsString(v);
  assert(v.toUpperCase() === "HI");

  const w: unknown = "abc";
  checks.assertIsString(w);
  assert(w.length === 3);

  const x: unknown = "xy";
  alias(x);
  assert(x.length === 2);

  const o: { n: number } | null = { n: 2 };
  assertPresent(o);
  assert(o.n === 2);
}
