// @target: es2015
// @strict: true
// @declaration: true
function ExpandoDecl(n: number): string {
    return n.toString();
}
ExpandoDecl.prop = 2
ExpandoDecl.m = function(n: number) {
    return n + 1;
}
let n = ExpandoDecl.prop + ExpandoDecl.m(12) + ExpandoDecl(101).length

const ExpandoExpr = function (n: number) {
    return n.toString();
}
ExpandoExpr.prop = { x: 2 }
ExpandoExpr.prop = { y: "" }
ExpandoExpr.m = function(n: number) {
    return n + 1;
}
let n_2 = (ExpandoExpr.prop.x || 0) + ExpandoExpr.m(12) + ExpandoExpr(101).length

const ExpandoArrow = (n: number) => n.toString();
ExpandoArrow.prop = 2
ExpandoArrow.m = function(n: number) {
    return n + 1;

}

/*pruned*/;                                                                 
                                         
                     
      
                                 
                  
 
/*pruned*/;             

function ExpandoMerge(n: number): number {
    return n * 100;
}
ExpandoMerge.p1 = 111
/*pruned*/;             
                        
 
/*pruned*/;             
                        
 
/*pruned*/;                                                                     

/*pruned*/;   
                                        
                             
                                                      
                                
     
 

// Should not work in Typescript -- must be const
let ExpandoExpr2 = function (n: number) {
    return n.toString();
}
ExpandoExpr2.prop = 2
ExpandoExpr2.m = function(n: number) {
    return n + 1;
}
let n_4 = ExpandoExpr2.prop + ExpandoExpr2.m(12) + ExpandoExpr2(101).length

// Should not work in typescript -- classes already have statics
class ExpandoClass {
    n: number = 1001;
}
ExpandoClass.prop = 2
ExpandoClass.m = function(n: number) {
    return n + 1;
}
let n_5 = ExpandoClass.prop + ExpandoClass.m(12) + new ExpandoClass().n

// Class expressions shouldn't work in typescript either
/*pruned*/;               
                      
 
/*pruned*/;          
/*pruned*/;                           
                 
 
/*pruned*/;                                                            



function main(): void {}
