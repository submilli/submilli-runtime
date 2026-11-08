// expect-error: does not satisfy the constraint
type Box<T extends number> = { value: T };
function main(): void { const value: Box<string> = { value: "bad" }; }
