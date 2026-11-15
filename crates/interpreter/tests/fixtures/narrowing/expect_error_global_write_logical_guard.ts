// expect-error-count: 6
// expect-error: cannot read field `length`
let shared: string | number = "a";
function change(): boolean { shared = 5; return false; }
class Getter {
  get value(): boolean { shared = 5; return true; }
}
function orGuard(): number {
  if (typeof shared !== "string" || change()) return 0;
  return shared.length;
}
function andGuard(): number {
  if (typeof shared === "string" && !change()) return shared.length;
  return 0;
}
function getterGuard(value: Getter): number {
  if (typeof shared === "string" && value.value) return shared.length;
  return 0;
}
function postfixGuard(value: { value: number }): number {
  if (typeof shared === "string" && value.value++ === 0) return shared.length;
  return 0;
}
function postfixIndexGuard(value: { value: number }, key: "value"): number {
  if (typeof shared === "string" && value[key]++ === 0) return shared.length;
  return 0;
}
function copiedGuardArgument(): number {
  if (((value: unknown): value is string => {
    change();
    return typeof value === "string";
  })(shared)) return shared.length;
  return 0;
}
function main(): number { return orGuard(); }
