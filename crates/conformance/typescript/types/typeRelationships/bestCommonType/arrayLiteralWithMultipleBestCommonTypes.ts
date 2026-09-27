// @target: es2015
// when multiple best common types exist we will choose the first candidate

let a: { x: number; y?: number } = null as unknown as ({ x: number; y?: number });
let b: { x: number; z?: number } = null as unknown as ({ x: number; z?: number });
let c: { x: number; a?: number } = null as unknown as ({ x: number; a?: number });

let as = [a, b]; // { x: number; y?: number };[]
let bs = [b, a]; // { x: number; z?: number };[]
let cs = [a, b, c]; // { x: number; y?: number };[]

let ds = [(x: Object) => 1, (x: string) => 2]; // { (x:Object) => number }[]
let es = [(x: string) => 2, (x: Object) => 1]; // { (x:string) => number }[]
let fs = [(a: { x: number; y?: number }) => 1, (b: { x: number; z?: number }) => 2]; // (a: { x: number; y?: number }) => number[]
let gs = [(b: { x: number; z?: number }) => 2, (a: { x: number; y?: number }) => 1]; // (b: { x: number; z?: number }) => number[]


function main(): void {}
