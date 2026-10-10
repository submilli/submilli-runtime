// expect-error: `let` declaration requires an initializer
// Type arguments must start on their type's line, as the `[]` suffix must.
let x: Array
<number> = [1];
function main(): void {}
