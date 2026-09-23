enum E { A = "a", B = "b" }
function f(x: E, a: "a"): boolean { return x === a; }
function main(): void { assert(f(E.A, "a"), "matching enum literal"); }
