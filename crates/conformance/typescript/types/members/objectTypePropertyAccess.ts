// @target: es2015
// Index notation should resolve to the type of a declared property with that same name
class C {
    foo: string;
}

/*pruned*/;                       
/*pruned*/;           
/*pruned*/;              
/*pruned*/;    
/*pruned*/;       

interface I {
    bar: string;
}
let i: I = null as unknown as (I);
let r4_2 = i.toString();
let r5 = i['toString']();
let r6 = i.bar;
let r7 = i['bar'];

let a = {
    foo: ''
}

let r8 = a.toString();
let r9 = a['toString']();
let r10 = a.foo;
let r11 = a['foo'];


function main(): void {}
