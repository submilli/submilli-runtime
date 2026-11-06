// expect-error: field
function main(): void { const value: Record<`id_${string}`, boolean> = {}; value["other"] = true; }
