//@module: commonjs
//@target: ES5, ES2015

// @filename: m1.ts
/*pruned*/;                                 
export function _(): void {
    console.log("_");
}
export function __(): void {
    console.log("__");
}
export function ___(): void {
    console.log("___");
}
export function _hi(): void {
    console.log("_hi");
}
export function __proto(): void {
    console.log("__proto");
}
export function __esmodule(): void {
    console.log("__esmodule");
}
export function ___hello(): void {
    console.log("___hello");
}

// @filename: m2.ts
import {_, __, ___hello, __esmodule, __proto, _hi}  from "./m1";
_();
__();
___hello();
__esmodule();
__proto();
_hi();