// expect-error: expected `"balance" | "owner"`, got `"secret"`
class Account { owner: string = "o"; private secret: number = 1; balance(): number { return this.secret; } }
function main(): void { const key: keyof Account = "secret"; }
