// @target: es2015
let x: "foo" = null as unknown as ("foo");
let y: "foo" | "bar" = null as unknown as ("foo" | "bar");

switch (x) {
    case "foo":
        break;
    case "bar":
        break;
    case y:
        y;
        break;
}


function main(): void {}
