type Result = [string, number] | [string, boolean];
function result(): Result { return ["ok", 1]; }
function flag(): Result { return ["ok", true]; }
function spread(): Result { const head: [string] = ["ok"]; return [...head, 2]; }
function shorter(): [string] | [string, boolean] { return ["short"]; }
function mixed(): [number] | [string] | number[] { const values: number[] = [1, 2]; return [...values]; }
function mixedTuple(): [string, number] | [number, string] | boolean[] { return ["ok", 2]; }
function callback(): [(x: number) => number, "n"] | [(x: string) => string, "s"] {
  return [(x: string): string => x.toUpperCase(), "s"];
}
function blockCallback(): [() => number, string] | [() => string, number] {
  return [() => { return 1; }, "x"];
}
function main(): void {
  const fn: (() => number) | (() => string) = () => { return 1; };
  assert(fn() === 1, "block callback actual return");
  const block = blockCallback();
  if (typeof block[1] === "string") assert(block[0]() === 1, "block callback correlation");
  assert(JSON.stringify(mixed()) === '[1,2]', "array alternative spread");
  assert(JSON.stringify(mixedTuple()) === '["ok",2]', "mixed tuple alternative");
  const c = callback();
  if (c[1] === "s") assert(c[0]("ok") === "OK", "callback context");
  assert(JSON.stringify(spread()) === '["ok",2]', "fixed tuple spread");
  assert(JSON.stringify(shorter()) === '["short"]', "different arity");
  assert(JSON.stringify(result()) === '["ok",1]', "numeric tuple");
  assert(JSON.stringify(flag()) === '["ok",true]', "boolean tuple");
  const pair: ["n", number] | ["b", boolean] = ["b", true];
  assert(JSON.stringify(pair) === '["b",true]', "correlated tuple");
}
