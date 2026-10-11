function consume(value: void): boolean { return value === undefined; }
function main(): void { assert(consume(undefined), "named void parameter"); }
