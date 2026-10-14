// @target: es2015
// @strict: true
/**/;    
                  
                    
                                         
                  
                       
 

/*pruned*/;                       
/*pruned*/;      
/*pruned*/;        
/*pruned*/;         
// BUG 817263
/*pruned*/;                              

interface I {
    "   ": number;
    "a   b": string;
    "~!@#$%^&*()_+{}|:'<>?\/.,`": number;
}

let i: I = null as unknown as (I);
let r_2 = i["   "];
/*pruned*/;          
let r3_2 = i["a   b"];
// BUG 817263
let r4_2 = i["~!@#$%^&*()_+{}|:'<>?\/.,`"];


let a: {
    "   ": number;
    "a   b": string;
    "~!@#$%^&*()_+{}|:'<>?\/.,`": number;
} = null as unknown as ({
    "   ": number;
    "a   b": string;
    "~!@#$%^&*()_+{}|:'<>?\/.,`": number;
});

let r_3 = a["   "];
/*pruned*/;          
let r3_3 = a["a   b"];
// BUG 817263
let r4_3 = a["~!@#$%^&*()_+{}|:'<>?\/.,`"];

let b = {
    "   ": 1,
    "a   b": "",
    "~!@#$%^&*()_+{}|:'<>?\/.,`": 1,
}

let r_4 = b["   "];
/*pruned*/;          
let r3_4 = b["a   b"];
// BUG 817263
let r4_4 = b["~!@#$%^&*()_+{}|:'<>?\/.,`"];


function main(): void {}
