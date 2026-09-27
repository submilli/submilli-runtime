// @target: es2015

type S = "a" | "b";
type T = S[] | S;

let s: S = null as unknown as (S);
let t: T = null as unknown as (T);
let str: string = null as unknown as (string);

////////////////

s = <S>t;
s = t as S;

s = <S>str;
s = str as S;

////////////////

t = <T>s;
t = s as T;

t = <T>str;
t = str as T;

////////////////

str = <string>s;
str = s as string;

str = <string>t;
str = t as string;


function main(): void {}
