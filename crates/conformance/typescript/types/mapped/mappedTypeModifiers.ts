// @target: es2015
// @strictNullChecks: true
// @noimplicitany: true

type T = { a: number, b: string };
type TP = { a?: number, b?: string };
type TR = { readonly a: number, readonly b: string };
type TPR = { readonly a?: number, readonly b?: string };

let v00: "a" | "b" = null as unknown as ("a" | "b");
let v00_2: keyof T = null as unknown as (keyof T);
let v00_3: keyof TP = null as unknown as (keyof TP);
let v00_4: keyof TR = null as unknown as (keyof TR);
let v00_5: keyof TPR = null as unknown as (keyof TPR);

let v01: T = null as unknown as (T);
/*pruned*/;                                                                         
let v01_3: Pick<T, keyof T> = null as unknown as (Pick<T, keyof T>);
let v01_4: Pick<Pick<T, keyof T>, keyof T> = null as unknown as (Pick<Pick<T, keyof T>, keyof T>);

let v02: TP = null as unknown as (TP);
/*pruned*/;                                                                           
let v02_3: Partial<T> = null as unknown as (Partial<T>);
/*pruned*/;                                                                             
let v02_5: Pick<TP, keyof TP> = null as unknown as (Pick<TP, keyof TP>);

let v03: TR = null as unknown as (TR);
/*pruned*/;                                                                                           
let v03_3: Readonly<T> = null as unknown as (Readonly<T>);
/*pruned*/;                                                                             
let v03_5: Pick<TR, keyof TR> = null as unknown as (Pick<TR, keyof TR>);

let v04: TPR = null as unknown as (TPR);
/*pruned*/;                                                                                             
let v04_3: Partial<TR> = null as unknown as (Partial<TR>);
let v04_4: Readonly<TP> = null as unknown as (Readonly<TP>);
let v04_5: Partial<Readonly<T>> = null as unknown as (Partial<Readonly<T>>);
let v04_6: Readonly<Partial<T>> = null as unknown as (Readonly<Partial<T>>);
/*pruned*/;                                                                                 
let v04_8: Pick<TPR, keyof T> = null as unknown as (Pick<TPR, keyof T>);

/*pruned*/;                                        

type B = { a: { x: number }, b: { x: string } };
type BP = { a?: { x: number }, b?: { x: string } };
type BR = { readonly a: { x: number }, readonly b: { x: string } };
type BPR = { readonly a?: { x: number }, readonly b?: { x: string } };

let b00: "a" | "b" = null as unknown as ("a" | "b");
let b00_2: keyof B = null as unknown as (keyof B);
let b00_3: keyof BP = null as unknown as (keyof BP);
let b00_4: keyof BR = null as unknown as (keyof BR);
let b00_5: keyof BPR = null as unknown as (keyof BPR);

let b01: B = null as unknown as (B);
/*pruned*/;                                                                         
let b01_3: Pick<B, keyof B> = null as unknown as (Pick<B, keyof B>);
let b01_4: Pick<Pick<B, keyof B>, keyof B> = null as unknown as (Pick<Pick<B, keyof B>, keyof B>);

let b02: BP = null as unknown as (BP);
/*pruned*/;                                                                           
let b02_3: Partial<B> = null as unknown as (Partial<B>);
/*pruned*/;                                                                             
let b02_5: Pick<BP, keyof BP> = null as unknown as (Pick<BP, keyof BP>);

let b03: BR = null as unknown as (BR);
/*pruned*/;                                                                                           
let b03_3: Readonly<B> = null as unknown as (Readonly<B>);
/*pruned*/;                                                                             
let b03_5: Pick<BR, keyof BR> = null as unknown as (Pick<BR, keyof BR>);

let b04: BPR = null as unknown as (BPR);
/*pruned*/;                                                                                             
let b04_3: Partial<BR> = null as unknown as (Partial<BR>);
let b04_4: Readonly<BP> = null as unknown as (Readonly<BP>);
let b04_5: Partial<Readonly<B>> = null as unknown as (Partial<Readonly<B>>);
let b04_6: Readonly<Partial<B>> = null as unknown as (Readonly<Partial<B>>);
/*pruned*/;                                                                                 
let b04_8: Pick<BPR, keyof BPR> = null as unknown as (Pick<BPR, keyof BPR>);

type Foo = { prop: number, [x: string]: number };

function f1(x: Partial<Foo>): void {
    x.prop; // ok
    (x["other"] || 0).toFixed();
}

function f2(x: Readonly<Foo>): void {
    x.prop; // ok
    x["other"].toFixed();
}

/*pruned*/;                          
                 
                           
 

/*pruned*/;                                         
                 
                         
 


function main(): void {}
