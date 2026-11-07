// expect-error-count: 1
// expect-error: expected `number`, got `number | string`
export function main(): void { const q = [1, "bad"]; const n: number = q[0]; }
