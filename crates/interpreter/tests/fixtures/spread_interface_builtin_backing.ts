interface WithLastIndex {
  lastIndex: number;
}

function main(): void {
  const original = /a/g;
  original.lastIndex = 1;
  const view: WithLastIndex = original;
  const copy = { ...view };
  assert(JSON.stringify(copy) === "{}", "built-in interface view has no enumerable fields");
}
