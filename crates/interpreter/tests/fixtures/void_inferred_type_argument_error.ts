// expect-error: type argument containing `void`
function f(): void {}
function main(): void { const r = [1, 2].map((x: number) => f() ?? 1); }
