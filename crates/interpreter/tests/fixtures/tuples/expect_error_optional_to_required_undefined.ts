// expect-error: expected `[number, string | undefined]`
function requiredView(value: [number, string?]): [number, string | undefined] { return value; }
function main(): void {}
