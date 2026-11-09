// @target: es2015
interface Runnable {
    isRunning: boolean;
}

interface Refrigerator extends Runnable {
    makesFoodGoBrrr: boolean;
}

let x: string = null as unknown as (string);
let y: "foo" | Refrigerator = null as unknown as ("foo" | Refrigerator);

let b: boolean = null as unknown as (boolean);
b = x == y;
b = "foo" == y
b = y == "foo";
b = "foo" == "bar";
b = "bar" == x;
b = x == "bar";
b = y == "bar";
b = "bar" == y;

b = x != y;
b = "foo" != y
b = y != "foo";
b = "foo" != "bar";
b = "bar" != x;
b = x != "bar";
b = y != "bar";
b = "bar" != y;


function main(): void {}
