// expect-error: expected `First`, got `Second | Third`
enum First { A = 2 }
enum Second { B = 3 }
enum Third { C = 4 }
function choose(flag: boolean): Second | Third { return flag ? Second.B : Third.C; }
function main(): void { console.log(First.A === choose(true)); }
