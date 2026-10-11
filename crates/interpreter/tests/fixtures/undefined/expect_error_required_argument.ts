// expect-error: expected 1 argument(s)
function required(value: string | undefined): void {}
function main(): void { required(); }
