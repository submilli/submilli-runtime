// @target: es5, es2015

let t1 = 10;
let t2 = 10;
/**/; 

// With TemplateTail
`${t1 ** t2} world`;
`${t1 ** t2 ** t1} world`;
`${t1 + t2 ** t1} world`;
`${t1 ** t2 + t1} world`;
`${t1 + t2 ** t2 + t1 } world`;
/*pruned*/;                         
/*pruned*/;                             

`${t1 ** t2}${t1 ** t2} world`;
`${t1 ** t2 ** t1}${t1 ** t2 ** t1} world`;
`${t1 + t2 ** t1}${t1 + t2 ** t1} world`;
`${t1 ** t2 + t1}${t1 ** t2 + t1} world`;
`${t1 + t2 ** t2 + t1}${t1 + t2 ** t2 + t1} world`;
/*pruned*/;                                                    

`${t1 ** t2} hello world ${t1 ** t2} !!`;
`${t1 ** t2 ** t1} hello world ${t1 ** t2 ** t1} !!`;
`${t1 + t2 ** t1} hello world ${t1 + t2 ** t1} !!`;
`${t1 ** t2 + t1} hello world ${t1 ** t2 + t1} !!`;
`${t1 + t2 ** t2 + t1} hello world ${t1 + t2 ** t2 + t1} !!`;
/*pruned*/;                                                             


function main(): void {}
