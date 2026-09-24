// @target: es2015
// Note that type guards affect types of variables and parameters only and 
// have no effect on members of objects such as properties. 

// variables in global
let num: number = null as unknown as (number);
let var1: string | number = null as unknown as (string | number);
class C1 {
    constructor(param: string | number) {
        // global vars in function declaration
        num = typeof var1 === "string" && var1.length; // string

        // variables in function declaration
        let var2: string | number = null as unknown as (string | number);
        num = typeof var2 === "string" && var2.length; // string

        // parameters in function declaration
        num = typeof param === "string" && param.length; // string
    }
    // Inside function declaration
    private p1(param: string | number): void {
        // global vars in function declaration
        num = typeof var1 === "string" && var1.length; // string

        // variables in function declaration
        let var2: string | number = null as unknown as (string | number);
        num = typeof var2 === "string" && var2.length; // string

        // parameters in function declaration
        num = typeof param === "string" && param.length; // string
    }
    // Inside function declaration
    p2(param: string | number): void {
        // global vars in function declaration
        num = typeof var1 === "string" && var1.length; // string

        // variables in function declaration
        let var2: string | number = null as unknown as (string | number);
        num = typeof var2 === "string" && var2.length; // string

        // parameters in function declaration
        num = typeof param === "string" && param.length; // string
    }
    // Inside function declaration
    private static s1(param: string | number): void {
        // global vars in function declaration
        num = typeof var1 === "string" && var1.length; // string

        // variables in function declaration
        let var2: string | number = null as unknown as (string | number);
        num = typeof var2 === "string" && var2.length; // string

        // parameters in function declaration
        num = typeof param === "string" && param.length; // string
    }
    // Inside function declaration
    static s2(param: string | number): void {
        // global vars in function declaration
        num = typeof var1 === "string" && var1.length; // string

        // variables in function declaration
        let var2: string | number = null as unknown as (string | number);
        num = typeof var2 === "string" && var2.length; // string

        // parameters in function declaration
        num = typeof param === "string" && param.length; // string
    }
}


function main(): void {}
