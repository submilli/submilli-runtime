function fallthrough(): undefined {}
function bare(): undefined { return; }
function maybe(flag: boolean): number | undefined { if (flag) { return 3; } }
function unknownFallthrough(): unknown { return; }
function main(): void {
  assert(fallthrough() === undefined, "undefined return fallthrough");
  assert(bare() === undefined, "bare return is undefined");
  assert(maybe(false) === undefined, "partial return falls through to undefined");
  assert(maybe(true) === 3, "explicit return retained");
  assert(unknownFallthrough() === undefined, "a bare return in an unknown function yields undefined");
}
