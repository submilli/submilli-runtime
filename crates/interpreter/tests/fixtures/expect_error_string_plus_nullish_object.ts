// expect-error: `+` not defined
function join(value: { a: number } | undefined): string { return "x" + value; }
function main(): void { join(undefined); }
