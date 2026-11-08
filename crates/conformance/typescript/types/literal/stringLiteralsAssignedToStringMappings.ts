// @target: es2015
let x: Uppercase<Lowercase<string>> = null as unknown as (Uppercase<Lowercase<string>>);

// good
x = "A";

// bad
x = "a";

let y: Uppercase<Lowercase<`${number}`>> = null as unknown as (Uppercase<Lowercase<`${number}`>>);

// good
y = "1";

// bad
y = "a";
y = "A";

function main(): void {}
