// expect-error-count: 14
// expect-error: cannot read field `length`
let shared: string | number = "a";
class Mutator {
  get value(): number { shared = 5; return 0; }
  set value(input: number) { shared = input; }
}
function direct(m: Mutator): number {
  if (typeof shared === "string") { const value = m.value; return shared.length + value; }
  return 0;
}
function indexed(m: Mutator): number {
  if (typeof shared === "string") { const value = m["value"]; return shared.length + value; }
  return 0;
}
function optional(m: Mutator | undefined): number {
  if (typeof shared === "string") { const value = m?.value; return shared.length; }
  return 0;
}
function setter(m: Mutator): number {
  if (typeof shared === "string") { m.value = 5; return shared.length; }
  return 0;
}
function structural(m: { value: number }): number {
  if (typeof shared === "string") { const value = m.value; return shared.length + value; }
  return 0;
}
function generic<T extends { value: number }>(m: T): number {
  if (typeof shared === "string") { const value = m.value; return shared.length + value; }
  return 0;
}
function genericIndex<T extends { value: number }, K extends keyof T>(m: T, key: K): number {
  if (typeof shared === "string") { const value = m[key]; return shared.length; }
  return 0;
}
function compoundGet(m: Mutator): void {
  if (typeof shared === "string") { m.value += shared.length; }
}
function compoundSet(m: Mutator): number {
  if (typeof shared === "string") { m.value += 1; return shared.length; }
  return 0;
}
function compoundIndex(m: { value: number }, key: "value"): void {
  if (typeof shared === "string") { m[key] += shared.length; }
}
function postfix(m: { value: number }): number {
  if (typeof shared === "string") { m.value++; return shared.length; }
  return 0;
}
function genericOptional<T extends { value: number }, K extends keyof T>(m: T | undefined, key: K): number {
  if (typeof shared === "string") { const value = m?.[key]; return shared.length; }
  return 0;
}
function genericWrite<T extends { value: number }, K extends keyof T>(m: T, key: K, value: T[K]): number {
  if (typeof shared === "string") { m[key] = value; return shared.length; }
  return 0;
}
function mapped<T extends { value: number }>(m: Partial<T>): number {
  if (typeof shared === "string") { const value = m.value; return shared.length; }
  return 0;
}
function main(): number { return direct(new Mutator()); }
