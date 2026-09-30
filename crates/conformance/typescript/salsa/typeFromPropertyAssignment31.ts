// @target: es2015
// @strict: true
function ExpandoMerge(n: number): number {
    return n;
}
ExpandoMerge.p1 = 111
ExpandoMerge.m = function(n: number) {
    return n + 1;
}
/*pruned*/;             
                        
 
ExpandoMerge.p4 = 44444; // ok
ExpandoMerge.p6 = 66666; // ok
ExpandoMerge.p8 = false; // type error
/*pruned*/;             
                        
                      
                      
                      
                      
                      
                      
 
ExpandoMerge.p5 = 555555; // ok
ExpandoMerge.p7 = 777777; // ok
ExpandoMerge.p9 = false; // type error
/*pruned*/;                                                                                                                                                                                                       


function main(): void {}
