// expect-error: field
function main(): void { const x: Record<number, boolean> = { other: true }; }
