// @target: es5, es2015

let t1 = 10;
let t2 = 10;
/**/; 

// With templateHead
`hello ${t1 ** t2}`;
`hello ${t1 ** t2 ** t1}`;
`hello ${t1 + t2 ** t1}`;
`hello ${t1 ** t2 + t1}`;
`hello ${t1 + t2 ** t2 + t1 }`;
/*pruned*/;                         
/*pruned*/;                             

`hello ${t1 ** t2}${t1 ** t2}`;
`hello ${t1 ** t2 ** t1}${t1 ** t2 ** t1}`;
`hello ${t1 + t2 ** t1}${t1 + t2 ** t1}`;
`hello ${t1 ** t2 + t1}${t1 ** t2 + t1}`;
`hello ${t1 + t2 ** t2 + t1}${t1 + t2 ** t2 + t1}`;
/*pruned*/;                                                    

`hello ${t1 ** t2} hello world ${t1 ** t2}`;
`hello ${t1 ** t2 ** t1} hello world ${t1 ** t2 ** t1}`;
`hello ${t1 + t2 ** t1} hello world ${t1 + t2 ** t1}`;
`hello ${t1 ** t2 + t1} hello world ${t1 ** t2 + t1}`;
`hello ${t1 + t2 ** t2 + t1} hello world ${t1 + t2 ** t2 + t1}`;
/*pruned*/;                                                                 

function main(): void {}
