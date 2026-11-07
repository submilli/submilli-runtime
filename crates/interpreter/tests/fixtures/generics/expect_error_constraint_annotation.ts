// expect-error: does not satisfy the constraint
interface Box<T extends number> { value: T; }
function main(): void { const box: Box<string> = { value: "wrong" }; }
