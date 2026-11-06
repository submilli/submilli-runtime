// expect-error: missing
function main(): void { const value: Record<number | "label", boolean> = {}; }
