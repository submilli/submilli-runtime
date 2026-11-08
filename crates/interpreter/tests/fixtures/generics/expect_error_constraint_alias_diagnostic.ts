// expect-error: non-object type `Id`
type Id = string | number;
function read(value: Id): string { return value.toFixed(2); }
function main(): void { read(1); }
