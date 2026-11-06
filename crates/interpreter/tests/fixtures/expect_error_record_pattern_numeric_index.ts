// expect-error: field
function main(): void { const x: Record<`${number}`, boolean> = {}; x[1] = true; }
