// expect-error: incompatible signature
interface One { m(n: string): number }
class Wrong implements One { m(n: number = 1, x: number = 2): number { return n+x; } }
function main(): void {}
