function main(): void {
  assert(RegExp("a+").test("aa"), "call without flags");
  const global = RegExp("a", "g");
  assert(global.test("aa") && global.lastIndex === 1, "global call advances lastIndex");
  assert(global.test("aa") && global.lastIndex === 2, "global call keeps state");

  const date = RegExp("([A-Za-z]+) (\\d{1,2}), (\\d{4})").exec("November 7, 2026");
  assert(date !== null && date[0] === "November 7, 2026", "issue reproduction");

  const original = /a/gi;
  original.lastIndex = 1;
  assert(RegExp(original) === original, "omitted flags preserve identity");
  assert(RegExp(original, undefined) === original, "undefined flags preserve identity");
  assert(original.lastIndex === 1, "identity call preserves lastIndex");
  const overridden = RegExp(original, "i");
  assert(overridden !== original && overridden.flags === "i", "flags create a new regex");
  assert(overridden.lastIndex === 0 && overridden.test("A"), "new regex starts at zero");
  const cleared = RegExp(original, "");
  assert(cleared.flags === "" && !cleared.test("A"), "empty flags override inherited flags");
  const sameFlags = RegExp(original, "gi");
  assert(sameFlags !== original && sameFlags.lastIndex === 0, "explicit identical flags still clone");

  const copy = new RegExp(original);
  assert(copy !== original && copy.source === original.source, "new always copies");
  assert(copy.flags === "gi" && copy.lastIndex === 0, "new inherits flags but resets lastIndex");
  const explicitUndefined = new RegExp(original, undefined);
  assert(explicitUndefined !== original && explicitUndefined.flags === "gi", "new with undefined copies");
  assert(new RegExp(original, "").flags === "", "new accepts a flags override");

  const pattern: string | RegExp = original;
  assert(RegExp(pattern) === original, "union input");
  const flags: string | undefined = undefined;
  assert(RegExp("a", flags).test("a"), "optional flags on string input");
  function make(pattern: string | RegExp, flags?: string): RegExp {
    return RegExp(pattern, flags);
  }
  assert(make(original) === original && make("a", "i").test("A"), "dynamic union and optional arguments");
  const alias = RegExp;
  assert(alias("b").test("b") && alias(original) === original, "constructor alias");
  interface Factory { (pattern: string | RegExp, flags?: string): RegExp; }
  const factory: Factory = RegExp;
  assert(factory("c").test("c") && factory(original) === original, "structural call signature");
  assert(factory(original, "").flags === "", "structural call with override");
  assert(factory("d", undefined).test("d"), "structural call with undefined flags");

  let order = "";
  function getPattern(): RegExp { order += "p"; return original; }
  function getFlags(): string { order += "f"; return "i"; }
  assert(RegExp(getPattern(), getFlags()).test("A") && order === "pf", "arguments run once in order");

  let invalidPattern = false;
  try { RegExp("["); } catch (error) { invalidPattern = error instanceof SyntaxError; }
  assert(invalidPattern, "invalid dynamic pattern throws SyntaxError");
  let invalidFlags = false;
  try { RegExp(original, "gg"); } catch (error) { invalidFlags = error instanceof SyntaxError; }
  assert(invalidFlags && original.lastIndex === 1, "invalid flags throw without changing original");
}
