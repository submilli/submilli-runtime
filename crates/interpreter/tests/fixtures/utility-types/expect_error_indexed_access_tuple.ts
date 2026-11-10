// expect-error: Tuple type `[number]` of length 1 has no element at index 5
function main(): void { const x: [number][5] = 1; }
