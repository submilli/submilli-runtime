// expect-error: expected 1 argument(s), got 0
// expect-error: function isNum(x: number | string): x is number
function isNum(x: number | string): x is number { return typeof x === "number"; }
function main(): void { isNum(); }
