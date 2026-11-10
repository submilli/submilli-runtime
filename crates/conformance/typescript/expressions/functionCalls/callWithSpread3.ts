// @target: es2015
const s2: [string, string] = null as unknown as ([string, string]);
const s3: [string, string, string] = null as unknown as ([string, string, string]);
/*pruned*/;                                                                                   
const s_: string[] = null as unknown as (string[]);
const n_: number[] = null as unknown as (number[]);
/*pruned*/;                                                                                    

function fs2(a: string, b: string): void { }
function fs2_(a: string, b: string, ...c: string[]): void { }
function fs2n_(a: string, b: string, ...c: number[]): void { }
function fs5(a: string, b: string, c: string, d: string, e: string): void { }

// error
fs2('a', ...s2); // error on ...s2
fs2('a', 'b', 'c', ...s2); // error on 'c' and ...s2
fs2('a', 'b', ...s2, 'c'); // error on ...s2 and 'c'
fs2('a', 'b', 'c', ...s2, 'd'); // error on 'c', ...s2 and 'd'
fs2(...s2, 'a'); // error on 'a'
fs2(...s3); // error on ...s3
fs2_(...s_); // error on ...s_
/*pruned*/;    // error on ...s2n_
fs2_(...s_, ...s_); // error on ...s_
fs2_(...s_, ...s_, ...s_); // error on ...s_
// fs2n_(...s2, ...s_); //           FIXME: should be a type error
/*pruned*/;    // error on ...s2_

// ok
/*pruned*/;  
/*pruned*/;         
/*pruned*/;          
/*pruned*/;         
/*pruned*/;    
fs2n_(...s2);
// fs2n_(...s2, ...n_); // FIXME: should compile
fs5(...s2, "foo", ...s2);


function main(): void {}
