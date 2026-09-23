// expect-error: expected
function f(x: "a" | "b"): boolean { return x === "c"; }
function main(): void { console.log(f("a")); }
