// expect-error: interface Box<T extends number>
interface Box<T extends number> { value: T; }
function main(): void { const box: Box = { value: 1 }; }
