// expect-error: tuple index 2 out of bounds
function item(value: [number, string] | null): string | null { return value?.[2]; }
function main(): void {}
