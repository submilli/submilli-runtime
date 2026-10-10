function nothing(): void {}
function id<T>(value: T): T { return value; }
function main(): void {
  assert(id(nothing()) === undefined, "generic inference preserves void value");
}
