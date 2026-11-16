function unused(value: { length: number }): number { return 3; }
function length(value: { length: number }): number { return value.length; }
function picked(value: Pick<String, "length">): number { return value.length; }
function id(value: Pick<string[], "length">): Pick<string[], "length"> { return value; }
function mapSize(value: Readonly<Map<string, string>>): number { return value.size; }
function main(): void {
  more();
  checked();
  const nums = [1];
  const push: Pick<number[], "push"> = nums;
  assert(push.push.length === 1);
  assert(push.push(2, 3) === 3);
  const slice: Pick<number[], "slice"> = nums;
  assert(slice.slice.length === 2);
  const native: Pick<number[], "splice" | "reduce"> = nums;
  assert(native.splice.length === 2);
  assert(native.reduce.length === 1);
  const mapper: Pick<number[], "map"> = nums;
  assert(JSON.stringify(mapper.map<string>((value) => `${value}`)) === '["1","2","3"]');
  assert(JSON.stringify(nums) === "[1,2,3]");
  const m: Readonly<Map<string,string>> = new Map<string,string>([["a", "1"]]);
  const get = m.get;
  assert(get === m.get);
  let failed = false;
  try { get("a"); } catch (e) { failed = e instanceof TypeError; }
  assert(failed);
  assert(m.get?.("a") === "1");
  assert(genericLength([1,2]) === 2);
  assert(genericChain([1,2]) === 2);
  assert(optionalMap(new Map<string,string>([["a", "1"]])) === "1");
  assert(optionalMap(undefined) === undefined);
  assert(unused("abc") === 3);
  assert(length("abc") === 3);
  assert(length([1, 2]) === 2);
  assert(length({ length: 4 }) === 4);
  assert(picked("abc") === 3);
  const parts = ["a", "b"];
  const p: Pick<string[], "length"> = parts;
  assert(JSON.stringify(id(p)) === '["a","b"]');
  assert(mapSize(new Map<string, string>([["a", "1"]])) === 1);
}
function mapRead(value: Readonly<Map<string, string>>): string | undefined { return value.get("a"); }
function optionalLength(value: { length: number } | undefined): number | undefined { return value?.length; }
function nullableLength(value: { length: number } | null): number | undefined { return value?.length; }
function more(): void {
  assert(mapRead(new Map<string, string>([["a", "1"]])) === "1");
  assert(optionalLength("abc") === 3);
  assert(optionalLength(undefined) === undefined);
  assert(nullableLength("abc") === 3);
  assert(nullableLength(null) === undefined);
  const array = [1, 2];
  const view: { length: number } = array;
  view.length--;
  assert(array.length === 1);
  const c: Partial<Console> = console;
  assert(keepConsole(c) === 7);
  assert(length("\ud800") === 1);
}

function keepConsole(value: Partial<Console>): number { return 7; }
function checked(): void {
 const text: unknown = "abc";
 assert((text as { length: number }).length === 3);
 const array: unknown = [1, 2];
 assert((array as { length: number }).length === 2);
 const map: unknown = new Map<string, string>([["a", "1"]]);
 assert((map as { size: number }).size === 1);
}

function optionalMap(value: Readonly<Map<string,string>> | undefined): string | undefined { return value?.get("a"); }
function genericLength<T extends number[]>(value: Pick<T, "length">): number { return value.length; }
function genericChain<T extends number[]>(value: Pick<T, "length"> | undefined): number { return value?.length ?? 0; }
