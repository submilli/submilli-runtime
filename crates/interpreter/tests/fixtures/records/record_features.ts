type Dictionary<T> = Record<string, T>;
interface Counts extends BaseCounts { total: number; }
interface BaseCounts { [key: string]: number; }
function read<T>(values: Dictionary<T>, key: string): T | undefined { return values[key]; }
function finite(key: "left" | "right"): number {
  const r: Record<"left" | "right", number> = { left: 1, right: 2 };
  r[key] = 4;
  return r[key];
}
function main(): string {
  const record: Counts = { total: 1 };
  const key: string = "next";
  record[key] = 3;
  assert(read<number>(record, key) === 3);
  assert(finite("left") === 4);
  const values: Dictionary<number | null> = { present: null };
  assert("present" in values);
  assert(!("absent" in values));
  values[key] = null;
  assert(key in values);
  assert(Object.keys(values).length === 2);
  const spread = { ...record };
  assert(spread[key] === 3);
  const computed = { [key]: 4, ...record, ["total"]: 9 };
  assert(computed[key] === 3);
  assert(computed.total === 9);
  const strings: Record<string, string> = { total: "replaced" };
  const overridden = { total: 1, ...strings };
  assert(overridden.total === "replaced");
  const computedSpread = { ["total"]: 1, ...strings };
  assert(computedSpread.total === "replaced");
  const lone = String.fromCharCode(55296);
  values[lone] = 7;
  assert(values[lone] === 7);
  const nested = JSON.parse('{"one":{"two":2}}') as Record<string, Record<string, number>>;
  assert(nested.one!.two === 2);
  return JSON.stringify(values);
}
