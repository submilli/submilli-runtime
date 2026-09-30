// expect-error: type argument containing `void`
function f(): void {}
function main(): void { const r = [1, 2].map((x: number) => (x === 1 ? f() : 1)); }
