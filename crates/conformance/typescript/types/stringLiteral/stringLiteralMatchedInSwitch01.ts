// @target: es2015

type S = "a" | "b";
type T = S[] | S;

let foo: T = null as unknown as (T);
switch (foo) {
    case "a":
    case "b":
        break;
    default:
        foo = (foo as S[])[0];
        break;
}

function main(): void {}
