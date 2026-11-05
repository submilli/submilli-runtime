// expect-error: field
function main(): void { const x: Record<`${string}${number}`, boolean> = {}; x["xx42"] = true; }
