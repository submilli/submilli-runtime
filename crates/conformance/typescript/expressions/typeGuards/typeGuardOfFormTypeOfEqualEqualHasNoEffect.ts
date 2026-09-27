// @target: es2015
class C { private p: string };

let strOrNum: string | number = null as unknown as (string | number);
let strOrBool: string | boolean = null as unknown as (string | boolean);
let numOrBool: number | boolean = null as unknown as (number | boolean);
/*pruned*/;                                              

// typeof x == s has not effect on typeguard
if (typeof strOrNum == "string") {
    let r1 = strOrNum; // string | number
}
else {
    let r1 = strOrNum; // string | number
}

if (typeof strOrBool == "boolean") {
    let r2 = strOrBool; // string | boolean
}
else {
    let r2 = strOrBool; // string | boolean
}

if (typeof numOrBool == "number") {
    let r3 = numOrBool; // number | boolean
}
else {
    let r3 =  numOrBool; // number | boolean
}

/*pruned*/;                     
                                  
 
      
                                  
 

function main(): void {}
