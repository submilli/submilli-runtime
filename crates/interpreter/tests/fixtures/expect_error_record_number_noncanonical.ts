// expect-error: field
function main(): void { const value: Record<number, boolean> = {}; value["01"] = true; }
