// @target: es2015
let stringOrNumber: string | number = null as unknown as (string | number);

if (typeof stringOrNumber === "number") {
    if (typeof stringOrNumber !== "number") {
        stringOrNumber;
    }
}

if (typeof stringOrNumber === "number" && typeof stringOrNumber !== "number") {
    stringOrNumber;
}


function main(): void {}
