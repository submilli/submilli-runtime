// expect-error-count: 1
// expect-error: expected `number[]`, got `string[]`
export function main(): void { const bad: string[] = ["bad"]; const q: number[] = [...bad]; }
