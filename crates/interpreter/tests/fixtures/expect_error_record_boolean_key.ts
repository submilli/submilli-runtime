// expect-error: Record keys must satisfy
function main(): void { const value: Record<boolean, string> = {}; }
