// @target: es2015
interface StrNum extends Array<string|number> {
    0: string;
    1: number;
    length: 2;
}

let x: [string, number] = null as unknown as ([string, number]);
let y: StrNum = null as unknown as (StrNum);
let z: {
    0: string;
    1: number;
    length: 2;
} = null as unknown as ({
    0: string;
    1: number;
    length: 2;
});

let [a, b, c] = x;
let [d, e, f] = y;
let [g, h, i] = z;
let j1: [number, number, number] = x;
let j2: [number, number, number] = y;
let j3: [number, number, number] = z;
let k1: [string, number, number] = x;
let k2: [string, number, number] = y;
let k3: [string, number, number] = z;
let l1: [number] = x;
let l2: [number] = y;
let l3: [number] = z;
let m1: [string] = x;
let m2: [string] = y;
let m3: [string] = z;
let n1: [number, string] = x;
let n2: [number, string] = y;
let n3: [number, string] = z;
let o1: [string, number] = x;
let o2: [string, number] = y;
let o3: [string, number] = y;


function main(): void {}
