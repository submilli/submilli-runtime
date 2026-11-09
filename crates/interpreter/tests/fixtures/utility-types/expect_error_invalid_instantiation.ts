// expect-error: `Partial` does not apply to `Uint8Array`
function part<T>(value: T): Partial<T> { return value; }
function main(): void { const p = part(new Uint8Array(2)); }
