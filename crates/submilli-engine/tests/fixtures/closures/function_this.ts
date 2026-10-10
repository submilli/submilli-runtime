function main(): void {
  const nestedAlias = { read: function(): number {
    const receiver = this;
    const inner = function(): number { return receiver.get(); };
    return inner();
  }, get: function() { return 47; } };
  assert(nestedAlias.read() === 47, "nested function captures receiver alias");
  const aliased = { read: function(): number { const receiver = this; return receiver.get(); }, get: function() { return 37; } };
  assert(aliased.read() === 37, "aliased receiver dependency");
  const indexed = { read: function(): number { return this?.["get"]() ?? 0; }, get: function() { return 41; } };
  assert(indexed.read() === 41, "optional indexed receiver dependency");
  const maybe: { value?: string } = {};
  const spread = { value: 43, ...maybe, read: function(): number | string { return this.value; } };
  assert(spread.read() === 43, "optional spread keeps earlier receiver field");
  const siblings = { read: function(): number { return this.get(); }, get: function() { return 29; } };
  assert(siblings.read() === 29, "later method supplies inferred return");
  const reversed = { read: function(): number { return this.value; }, value: 17 };
  assert(reversed.read() === 17, "later field supplies receiver type");
  const obj = { value: 7, read: function(): number { return this.value; } };
  assert(obj.read() === 7, "object receiver");
  assert((obj.read as () => number)() === 7, "cast preserves reference");
  assert(obj.read!() === 7, "non-null assertion preserves reference");
  assert((obj?.read)!() === 7, "parenthesized chain preserves reference");
  assert(obj?.read!() === 7, "chain assertion preserves reference");
  const other = { value: 9, read: obj.read };
  assert(other.read() === 9, "call-bound receiver");
  const escaped = { value: 23, make: function(): () => number { return (): number => this.value; } };
  const arrowHolder = { value: 99, read: escaped.make() };
  assert(arrowHolder.read() === 23, "escaped arrow keeps lexical receiver");
  const independent = { value: 1, read: function(use: boolean): number {
    if (use) { return this.value; }
    return 31;
  } };
  const unusedThis = independent.read;
  assert(unusedThis(false) === 31, "unbound call without this access is valid");
  const read = obj.read;
  let threw = false;
  try { read(); } catch (e: Error) { threw = e instanceof TypeError; }
  assert(threw, "unbound this throws");
  const explicit = function(this: { value: number }): number { return this.value; };
  const bound = { value: 11, read: explicit };
  assert(bound.read() === 11, "explicit receiver annotation");
  const lexical = { value: 13, read: function(): number { const arrow = (): number => this.value; return arrow(); } };
  assert(lexical.read() === 13, "nested arrow captures receiver");
  const adapted: { value: number; run: () => void } = { value: 0, run: function(): number { this.value += 1; return this.value; } };
  adapted.run();
  assert(adapted.value === 1, "void adapter forwards receiver");
  const optional: { value: number; read: () => number } | null = obj;
  assert(optional?.read() === 7, "optional receiver");
  assert(obj.read?.() === 7, "optional call");
}
