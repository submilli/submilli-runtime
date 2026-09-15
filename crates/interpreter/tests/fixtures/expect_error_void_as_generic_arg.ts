// expect-error: got `void`
function nothing(): void {}
function id<T>(x: T): T { return x; }

function main(): void {
  id(nothing());
}
