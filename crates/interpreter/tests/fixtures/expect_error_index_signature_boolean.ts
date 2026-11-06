// expect-error-count: 1
// expect-error: index signature keys
function main(): void { const value: { [key: boolean]: string } = {}; }
