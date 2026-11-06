// expect-error: unsupported Record key type
function main(): void { const value: Record<{ id: number }, string> = {}; }
