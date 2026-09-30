// @target: es2015
// @strict: true
let str: string = null as unknown as (string);
let num: number = null as unknown as (number);
let strOrNumber: string | number = str || num;
let objStr: { prop: string } = null as unknown as ({ prop: string });
let objNum: { prop: number } = null as unknown as ({ prop: number });
let objStrOrNum1: { prop: string } | { prop: number } = objStr || objNum;
let objStrOrNum2: { prop: string | number } = objStr || objNum;
// Below is error because :
// Spec says:
// S is a union type and each constituent type of S is assignable to T.
// T is a union type and S is assignable to at least one constituent type of T.
// In case of objStrOrNum3, the S is not union Type but object Literal so we go to next step. 
// Since T is union Type we only allow the assignment of either object with property of type string or object with property of type number but do not allow object with property of type string | number
let objStrOrNum3: { prop: string } | { prop: number } = {
    prop: strOrNumber
};
let objStrOrNum4: { prop: string | number } = {
    prop: strOrNumber
};
let objStrOrNum5: { prop: string; anotherP: string; } | { prop: number } = { prop: strOrNumber };
let objStrOrNum6: { prop: string; anotherP: string; } | { prop: number } = {
    prop: strOrNumber,
    anotherP: str
};
let objStrOrNum7: { prop: string; anotherP: string; } | { prop: number; anotherP1: number } = {
    prop: strOrNumber,
    anotherP: str
};
let objStrOrNum8: { prop: string; anotherP: string; } | { prop: number; anotherP1: number } = {
    prop: strOrNumber,
    anotherP: str,
    anotherP1: num
};
interface I11 {
    commonMethodDifferentReturnType(a: string, b: number): string;
}
interface I21 {
    commonMethodDifferentReturnType(a: string, b: number): number;
}
/*pruned*/;                             
/*pruned*/;                             
/*pruned*/;                   
/*pruned*/;                     
/*pruned*/;                             
                                                
                            
                 
      
  
/*pruned*/;                             
                                                
                                
                 
      
  
let strOrNumber_2: string | number = null as unknown as (string | number);
let i11Ori21_5: I11 | I21 = { // Like i1 and i2 both
    commonMethodDifferentReturnType: (a, b) => strOrNumber,
};

function main(): void {}
