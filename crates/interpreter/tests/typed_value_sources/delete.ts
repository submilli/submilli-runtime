interface Optional { p?: number; keep: number }
function remove(o: Optional): boolean { return delete o.p; }
function absent(o: Optional): boolean {
  if (o.p !== undefined || "p" in o || Object.hasOwn(o, "p")) return false;
  if (Object.keys(o).join(",") !== "keep") return false;
  if (JSON.stringify(o) !== '{"keep":2}') return false;
  if (JSON.stringify({ ...o }) !== '{"keep":2}') return false;
  let keys = "";
  for (const k in o) keys += k;
  return keys === "keep";
}
class RequiredFields { p: number = 1; keep: number = 2; }
function main(): number {
  const optional: Optional = { p: 1, keep: 2 };
  const sibling: Optional = { p: 7, keep: 2 };
  if (!remove(optional) || !absent(optional) || sibling.p !== 7) return 1;
  if (!remove(optional) || !absent(optional)) return 2;
  optional.p = 9;
  if (optional.p !== 9 || !("p" in optional)) return 3;
  if (!remove(optional) || !absent(optional)) return 4;
  const required = { p: 1, keep: 2 };
  const other = { p: 1, keep: 2 };
  if (!remove(required) || !absent(required) || other.p !== 1) return 5;
  if (required.p !== undefined) return 6;
  const parsed = JSON.parse('{"p":1,"keep":2}') as Optional;
  if (!remove(parsed) || !absent(parsed)) return 7;
  const spread = { ...sibling };
  if (!remove(spread) || !absent(spread) || sibling.p !== 7) return 8;
  const instance = new RequiredFields();
  if (!remove(instance) || !absent(instance)) return 9;
  const missing: Optional = { keep: 2 };
  if (!remove(missing) || !absent(missing)) return 10;
  let calls = 0;
  function receiver(): Optional { calls++; return sibling; }
  const f = (): boolean => delete receiver()["p"];
  if (!f() || calls !== 1 || !absent(sibling)) return 11;
  return 0;
}
