// expect-error: object literal is missing required field `age`
interface User { name: string; age?: number }
function main(): void { const user: Required<User> = { name: "n" }; }
