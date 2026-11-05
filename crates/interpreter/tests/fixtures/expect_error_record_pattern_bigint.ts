// expect-error: field
function main(): void { const x: Record<`${bigint}`, boolean> = {}; x["01"] = true; }
