function s(b: boolean): string | null { return b ? "x" : null; }
function b(v: boolean): boolean | null { return v ? true : null; }
function value(flag: boolean): number | null { return flag ? 2 : null; }
function main(): void {
 assert("x" === s(true), "string"); assert("x" !== s(false), "string null");
 assert(true === b(true), "boolean"); assert(false !== b(false), "boolean null");
 const n = value(true);
 const empty = value(false);
 assert(2 === n && n === 2, "number equality");
 assert(2 !== empty && empty !== 2, "null inequality");
}
