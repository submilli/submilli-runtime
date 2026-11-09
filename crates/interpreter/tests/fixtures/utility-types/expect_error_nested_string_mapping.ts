// expect-error: expected `Uppercase<Lowercase<string>>`, got `Uppercase<string>`
function narrow(loud: Uppercase<string>): Uppercase<Lowercase<string>> { return loud; }
function main(): void {}
