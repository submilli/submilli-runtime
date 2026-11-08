// expect-error: Property `nope` does not exist on type `User`
interface User { name: string }
function main(): void { const x: User["nope"] = 1; }
