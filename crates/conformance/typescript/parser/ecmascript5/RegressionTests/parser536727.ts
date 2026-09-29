// @target: es2015
function foo(f: (x: string) => string): string {
    return f("");
}
let g = (x: string) => x + "blah";
let x = () => g;
foo(g);
foo(() => g);
foo(x);


function main(): void {}
