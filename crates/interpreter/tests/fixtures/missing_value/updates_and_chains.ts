function stringChain<T>(xs: (T | string)[], s: string): T | string {
  let v = xs[0];
  v = s?.[9];
  return v;
}
function byteChain<T>(xs: (T | number)[], bytes: Uint8Array): T | number {
  let v = xs[0];
  v = bytes?.[9];
  return v;
}
function chain<V>(rs: Record<string, V>[], fallback: V): string {
  const x: V | undefined = rs[0]?.foo;
  return `${x === undefined}:${x ?? fallback}:${typeof x}`;
}
function error(f: () => void): string {
  try { f(); } catch (e: MissingValueError) { return e.message; }
  return "no error";
}
function main(): void {
  assert(error(() => { stringChain<number>([1], "a"); }).includes("missing"), "missing string chain");
  assert(error(() => { byteChain<string>([1], new Uint8Array(1)); }).includes("missing"), "missing byte chain");
  const r: Record<string, number> = { k: 2 };
  const k = "k";
  assert(r.k++ === 2, "dot postfix result");
  r.k += 4;
  assert(r[k]++ === 7, "index postfix result");
  r[k] -= 3;
  assert(r.k === 5, "updates stored");
  let receivers = 0;
  let keys = 0;
  const receiver = (): Record<string, number> => { receivers++; return r; };
  const key = (): string => { keys++; return k; };
  assert(receiver()[key()]++ === 5, "old value with effects");
  receiver()[key()] -= 2;
  assert(receivers === 2 && keys === 2 && r.k === 4, "evaluate once");
  const empty: Record<string, number> = {};
  assert(error(() => { empty.k++; }).includes("key"), "missing dot increment");
  assert(error(() => { empty.k += 1; }).includes("key"), "missing dot compound");
  assert(error(() => { empty[k]++; }).includes("key"), "missing index increment");
  assert(error(() => { empty[k] -= 1; }).includes("key"), "missing index compound");
  assert(chain<number>([], 9) === "true:9:undefined", "missing element");
  assert(chain<number>([{}], 9) === "true:9:undefined", "missing key");
  assert(chain<number>([{foo: 2}], 9) === "false:2:number", "present value");
  assert(chain<number | undefined>([{foo: undefined}], 9) === "true:9:undefined", "present undefined");
  const m = /(a)?b/.exec("b");
  assert(error(() => { String(m?.[1].length); }).includes("group 1 of `m` did not take part"), "missing group");
  const present = /(a)?b/.exec("ab");
  assert(String(present?.[1].length) === "1", "present group");
  const absent = /(a)?b/.exec("z");
  assert(String(absent?.[1].length) === "undefined", "null match");
}
