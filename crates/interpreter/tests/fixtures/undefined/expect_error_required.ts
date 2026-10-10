// expect-error: missing required field `a`
function main(): void { const value: { a: string | undefined } = {}; }
