// @target: es5, es2015

let t1 = 10;
let t2 = 10;
/**/; 

// With TemplateTail
`${t1 ** -t2} world`;
`${(-t1) ** t2 - t1} world`;
/*pruned*/;                   
`${(-t1++) ** t2 - t1} world`;
/*pruned*/;                     
/*pruned*/;                         

// TempateHead & TemplateTail are empt
`${t1 ** -t2} hello world ${t1 ** -t2}`;
`${(-t1) ** t2 - t1} hello world ${(-t1) ** t2 - t1}`;
/*pruned*/;                                                 
`${(-t1++) ** t2 - t1} hello world ${t2 ** (-t1++) **  - t1}`;
/*pruned*/;                                                   
/*pruned*/;                                                         

// With templateHead
`hello ${(-t1) ** t2 - t1}`;
/*pruned*/;                   
`hello ${(-t1++) ** t2 - t1}`;
/*pruned*/;                     
/*pruned*/;                        

function main(): void {}
