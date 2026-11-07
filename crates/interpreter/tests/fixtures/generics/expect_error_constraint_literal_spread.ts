// expect-error: got `string[]`
function count<T extends "a" | "b">(...values: T[]): number { return values.length; }
function main(): void { count(...["a", "b"]); }
