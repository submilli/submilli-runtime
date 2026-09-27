// expect-error: expected `boolean`, got `number`
function main(): ["n", number] | ["b", boolean] { return ["b", 1]; }
