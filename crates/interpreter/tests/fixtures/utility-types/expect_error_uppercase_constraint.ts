// expect-error: Type `number` does not satisfy the constraint `string`
function main(): void { const x: Uppercase<number> = "1"; }
