// expect-error: does not satisfy
function main(): void { const value: Record<number, boolean> = {}; const key: string = "no"; value[key] = true; }
