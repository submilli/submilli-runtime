let calls = 0;
function makePair(): [number, string] {
  calls += 1;
  return [4, "x"];
}

function pair(a: number, b: string): string {
  return `${a}:${b}`;
}

function triple(a: number, b: number, c: string): string {
  return `${a}:${b}:${c}`;
}

function fixedThenRest(a: number, ...rest: string[]): string {
  return `${a}:${rest.join(":")}`;
}

function genericPair<T>(a: T, b: string): string {
  return `${a}:${b}`;
}

function optionalMiddle(a: number, b?: string, ...rest: string[]): string {
  return `${a}|${b}|${rest.join(",")}`;
}

function genericOptionalMiddle<T>(a: T, b?: string, ...rest: string[]): string {
  return `${a}|${b}|${rest.join(",")}`;
}

function defaulted(a: number = 7): number {
  return a;
}

function main(): void {
  const tuple: [number, string] = [2, "a"];
  assert(pair(...tuple) === "2:a", "tuple variable spread");
  assert(pair(...makePair()) === "4:x", "tuple source spread");
  assert(calls === 1, "tuple source evaluated once");
  assert(triple(1, ...tuple) === "1:2:a", "tuple spread after an ordinary argument");
  assert(fixedThenRest(...tuple) === "2:a", "tuple covers fixed and rest parameters");
  assert(genericPair(...tuple) === "2:a", "tuple into generic fixed parameters");
  const powers: [number, number] = [2, 3];
  assert(Math.pow(...powers) === 8, "tuple into namespace function");
  const optional: ((a: number, b: string) => string) | undefined = pair;
  assert(optional?.(...tuple) === "2:a", "tuple into optional call");
  const text: [string] = ['{"value":7}'];
  assert(String(...text) === text[0], "tuple into intrinsic");
  const parsed = JSON.parse(...text) as { value: number };
  assert(parsed.value === 7, "tuple into JSON.parse");
  const object: [{ value: number }] = [{ value: 7 }];
  assert(JSON.stringify(...object) === '{"value":7}', "tuple into JSON.stringify");
  const short: [number, string?] = [1];
  const long: [number, string?] = [1, "b"];
  assert(optionalMiddle(...short, "x") === "1|x|", "omitted tuple element shifts following argument");
  assert(optionalMiddle(...long, "x") === "1|b|x", "present tuple element retains position");
  assert(genericOptionalMiddle(...short, "x") === "1|x|", "generic optional tuple shifts arguments");
  const mutable: [number, string] = [1, "b"];
  const alias = mutable as (number | string)[];
  alias.pop();
  assert(optionalMiddle(...mutable) === "1|undefined|", "shortened tuple omits optional slot");
  alias.push("c", "d");
  assert(optionalMiddle(...mutable) === "1|c|d", "grown tuple retains extra rest elements");
  const missing: [number?] = [];
  assert(defaulted(...missing) === 7, "optional tuple preserves parameter default");
  const jsonOptional: [{ value: number }, undefined?] = [{ value: 1 }];
  assert(JSON.stringify(...jsonOptional, 2) === '{"value":1}', "JSON.stringify respects runtime argument positions");
}
