// expect-error: expected `T`
function unsound<T extends { id: number }>(value: T): T { return { id: 1 }; }
function main(): void {}
