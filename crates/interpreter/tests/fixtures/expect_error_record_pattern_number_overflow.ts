// expect-error: field
function main(): void { const x: Record<`n_${number}`, boolean> = {}; x["n_1e999"] = true; }
