// A later module-level variable whose type would come from inferring its
// initializer can't be used above it yet; annotating it lifts that. The error is
// at the declaration, so a use in a loop condition, which is inferred more than
// once, can't lose it.
// expect-error: `items` is used by a function above its declaration, so its type must be written
// expect-error: `limit` is used by a function above its declaration, so its type must be written
// expect-error-count: 2
const size = (): number => items.length;
const pushers: (() => number)[] = [];
while (pushers.push((): number => limit[0]) < 2) {}
const items = [1, 2];
const limit = [3];
function main(): void {}
