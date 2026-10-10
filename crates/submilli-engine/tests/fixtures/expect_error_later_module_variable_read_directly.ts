// Only a function body may use a later module-level variable. Code at the top
// level runs before the declaration, as TypeScript reports too (TS2448).
// expect-error: unresolved identifier `count`
// expect-error-count: 2
const read = (): number => count;
const early = count;
count = 3;
let count = 1;
function main(): void {}
