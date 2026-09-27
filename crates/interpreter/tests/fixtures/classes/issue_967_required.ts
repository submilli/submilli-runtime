// expect-error: incompatible signature
interface Zero { m(): number }
class RequiredArgument implements Zero { m(n: number): number { return n; } }
function main(): void {}
