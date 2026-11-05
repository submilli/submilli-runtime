// expect-error: string
function main(): void { const key: keyof Record<string, boolean> = 1; }
