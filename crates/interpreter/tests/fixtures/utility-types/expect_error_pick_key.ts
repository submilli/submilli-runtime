// expect-error: Type `"nope"` does not satisfy the constraint `keyof User`
interface User { name: string }
function main(): void { const user: Pick<User, "nope"> = {}; }
