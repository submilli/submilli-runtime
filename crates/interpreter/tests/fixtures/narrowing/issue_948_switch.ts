function nested<T>(x: T): T {
 if (typeof x === "string") {
   switch (x) {
    case "a": return x;
    default: return x;
   }
 }
 return x;
}
export function main(): void { assert(nested("a") === "a"); assert(nested("b") === "b"); }
