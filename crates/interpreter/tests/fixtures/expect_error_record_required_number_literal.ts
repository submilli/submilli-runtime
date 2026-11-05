// expect-error: missing
function main(): void { const value: Record<1 | 2, boolean> = { "1": true }; }
