function isPrimitive(x: unknown): x is string | number {
 if (typeof x === "string") { return true; }
 if (typeof x === "number") { return true; }
 return false;
}
function nested<T>(x: T): T {
 if (isPrimitive(x)) {
   if (typeof x === "string") { assert(x.length === 2); return x; }
   return x;
 }
 return x;
}
export function main(): void { assert(nested("hi") === "hi"); assert(nested(3) === 3); }
