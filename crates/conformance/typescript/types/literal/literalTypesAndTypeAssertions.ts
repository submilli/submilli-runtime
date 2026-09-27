// @target: es2015
const obj = {
    a: "foo" as "foo",
    b: <"foo">"foo",
    c: "foo"
};

let x1 = 1 as (0 | 1);
let x2 = 1;

/*pruned*/;                      
/*pruned*/;                               
/*pruned*/;                               
/*pruned*/;                                        


function main(): void {}
