// @target: es5, es2015

let t1 = 10;
let t2 = 10;
/**/; 

// Error: early syntax error using ES7 SimpleUnaryExpression on left-hand side without ()
// With TemplateTail
`${-t1 ** t2 - t1} world`;
/*pruned*/;                 
`${-t1++ ** t2 - t1} world`;
`${!t1 ** t2 ** --t1 } world`;
`${typeof t1 ** t2 ** t1} world`;
`${1 + typeof t1 ** t2 ** t1} world`;

`${-t1 ** t2 - t1}${-t1 ** t2 - t1} world`;
/*pruned*/;                                    
`${-t1++ ** t2 - t1}${-t1++ ** t2 - t1} world`;
`${!t1 ** t2 ** --t1 }${!t1 ** t2 ** --t1 } world`;
`${typeof t1 ** t2 ** t1}${typeof t1 ** t2 ** t1} world`;
`${1 + typeof t1 ** t2 ** t1}${1 + typeof t1 ** t2 ** t1} world`;

`${-t1 ** t2 - t1} hello world ${-t1 ** t2 - t1} !!`;
/*pruned*/;                                              
`${-t1++ ** t2 - t1} hello world ${-t1++ ** t2 - t1} !!`;
`${!t1 ** t2 ** --t1 } hello world ${!t1 ** t2 ** --t1 } !!`;
`${typeof t1 ** t2 ** t1} hello world ${typeof t1 ** t2 ** t1} !!`;
`${1 + typeof t1 ** t2 ** t1} hello world ${1 + typeof t1 ** t2 ** t1} !!`;

function main(): void {}
