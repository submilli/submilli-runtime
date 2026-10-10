// @target: es2015
// @strictNullChecks: true
// @allowUnreachableCode: false
function f1(
  required: unknown = (() => {
    throw new Error("bad");
  })()
): void {
  console.log("ok"); // should not trigger 'Unreachable code detected.'
}

function f2(
  a: number | string | undefined,
  required: unknown = (() => {
    a = 1;
  })()
): void {
  a; // should be number | string | undefined
}

function f3(
  a: number | string | undefined = 1,
  required: unknown = (() => {
    a = "";
  })()
): void {
  a; // should be number | string
}

/*pruned*/; 
                                     
                               
         
                        
 


function main(): void {}
