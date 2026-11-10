// @target: es2015
// no errors here

class C {
    1: number;
    1.1: string;
}

/*pruned*/;                       
/*pruned*/;   
/*pruned*/;     
/*pruned*/;     
/*pruned*/;       

interface I {
    1: number;
    1.1: string;
}

let i: I = null as unknown as (I);
let r1_2 = i[1];
let r2_2 = i[1.1];
let r3_2 = i['1'];
let r4_2 = i['1.1'];

let a: {
    1: number;
    1.1: string;
} = null as unknown as ({
    1: number;
    1.1: string;
});

let r1_3 = a[1];
let r2_3 = a[1.1];
let r3_3 = a['1'];
let r4_3 = a['1.1'];

let b = {
    1: 1,
    1.1: ""
}

let r1_4 = b[1];
let r2_4 = b[1.1];
let r3_4 = b['1'];
let r4_4 = b['1.1'];

function main(): void {}
