// @target: es2015
interface I1<T> {
    commonMethodType(a: string): string;
    commonPropertyType: string;

    commonMethodDifferentParameterType(a: string): string;
    commonMethodDifferentReturnType(a: string): string;
    commonPropertyDifferenType: string;

    commonMethodWithTypeParameter(a: T): T;
    commonMethodWithOwnTypeParameter<U>(a: U): U;

    methodOnlyInI1(a: string): string;
    propertyOnlyInI1: string;
}

interface I2<T> {
    commonMethodType(a: string): string;
    commonPropertyType: string;

    commonMethodDifferentParameterType(a: number): number;
    commonMethodDifferentReturnType(a: string): number;
    commonPropertyDifferenType: number;

    commonMethodWithTypeParameter(a: T): T;
    commonMethodWithOwnTypeParameter<U>(a: U): U;

    methodOnlyInI2(a: string): string;
    propertyOnlyInI2: string;
}

// a union type U has those members that are present in every one of its constituent types, 
// with types that are unions of the respective members in the constituent types
/*pruned*/;                                                                    
let str: string = null as unknown as (string);
let num: number = null as unknown as (number);
let strOrNum: string | number = null as unknown as (string | number);

// If each type in U has a property P, U has a property P of a union type of the types of P from each type in U.
/*pruned*/;                 // string
/*pruned*/;                    // (a: string) => string so result should be string
/*pruned*/;                             
/*pruned*/;                                        // string | union
/*pruned*/;                           // No error - property exists
/*pruned*/;                                     // error - no call signatures because the type of this property is ((a: string) => string) | (a: number) => number
                                                // and the call signatures arent identical
/*pruned*/;                                
/*pruned*/;                                   
/*pruned*/;                                   
/*pruned*/;                                             

/*pruned*/;         // error
/*pruned*/;         // error
/*pruned*/;                // error
/*pruned*/;           // error

function main(): void {}
