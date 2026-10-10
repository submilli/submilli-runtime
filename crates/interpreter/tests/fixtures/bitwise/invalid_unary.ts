// expect-error: cannot apply unary arithmetic to `unknown`
// expect-error: unary `~` not defined for `null`
function reject(value: unknown): void { const result = ~value; }
function main(): void { const result = ~null; }
