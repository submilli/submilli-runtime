function nothing(): void {}
function maybe(): number | null { return null; }
function main(): void {
  let matched = false;
  switch (maybe() ?? nothing()) { case undefined: matched = true; break; }
  assert(matched, "conditional void value matches undefined");
  assert((true ? nothing() : 1) === undefined, "conditional void equality");
}
