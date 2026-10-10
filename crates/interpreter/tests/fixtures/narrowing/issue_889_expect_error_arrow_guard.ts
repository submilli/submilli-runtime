// expect-error: type-guard return value does not narrow parameter
const bad = (x: number | string): x is number => true;
export function main(): void {}
