// @target: es2015
class A { a: string; }
class B { b: number; }
class C { b: Object; }
/*pruned*/;         

function namedClasses(x: A | B): void {
    if ("a" in x) {
        x.a = "1";
    } else {
        x.b = 1;
    }
}

/*pruned*/;                                       
                   
                                   
            
                                     
     
 

function anonymousClasses(x: { a: string; } | { b: number; }): void {
    if ("a" in x) {
        let y: string = x.a;
    } else {
        let z: number = x.b;
    }
}

class AWithOptionalProp { a?: string; }
class BWithOptionalProp { b?: string; }

function positiveTestClassesWithOptionalProperties(x: AWithOptionalProp | BWithOptionalProp): void {
    if ("a" in x) {
        x.a = "1";
    } else {
        const y: string = x instanceof AWithOptionalProp
            ? x.a
            : x.b
    }
}

function inParenthesizedExpression(x: A | B): void {
    if ("a" in (x)) {
        let y: string = x.a;
    } else {
        let z: number = x.b;
    }
}

class ClassWithUnionProp { prop: A | B; }

function inProperty(x: ClassWithUnionProp): void {
    if ("a" in x.prop) {
        let y: string = x.prop.a;
    } else {
        let z: number = x.prop.b;
    }
}

class NestedClassWithProp { outer: ClassWithUnionProp; }

function innestedProperty(x: NestedClassWithProp): void {
    if ("a" in x.outer.prop) {
        let y: string = x.outer.prop.a;
    } else {
        let z: number = x.outer.prop.b;
    }
}

/*pruned*/;            
                          
                    
                               
                                        
                
                                        
         
     
 

// added for completeness
class SelfAssert {
    a: string;
    inThis(): void {
        if ("a" in this) {
            let y: string = this.a;
        } else {
        }
    }
}

/*pruned*/;        
                     
 

/*pruned*/;                  
                   
                   
     
                        
                   
     
                           
 


function main(): void {}
