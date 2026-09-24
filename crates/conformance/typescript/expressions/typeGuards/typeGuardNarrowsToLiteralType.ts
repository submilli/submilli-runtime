// @target: es2015
function isFoo(value: string) : value is "foo" { return null as unknown as (boolean); }
function doThis(value: "foo"): void { }
function doThat(value: string) : void { }
let value: string = null as unknown as (string);
if (isFoo(value)) {
    doThis(value);
} else {
    doThat(value);
}



function main(): void {}
