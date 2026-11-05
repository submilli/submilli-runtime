// expect-error: readonly
function main(): void { const xs: readonly number[] = [1]; xs["0"] = 2; }
