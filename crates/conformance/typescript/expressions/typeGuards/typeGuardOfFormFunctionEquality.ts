// @target: es2015
function isString1(a: number, b: Object): b is string { return null as unknown as (boolean); }

function isString2(a: Object): a is string { return null as unknown as (boolean); }

/*pruned*/;                
                       
            
 

let x = isString1(0, "") === isString2("");

function isString3(a: number, b: number, c: Object): c is string {
    return isString1(0, c);
}


function main(): void {}
