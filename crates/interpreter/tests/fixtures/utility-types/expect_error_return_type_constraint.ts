// expect-error: Type `number` does not satisfy the constraint `(...arg0: never[]) => unknown`
function main(): void { const x: ReturnType<number> = 1; }
