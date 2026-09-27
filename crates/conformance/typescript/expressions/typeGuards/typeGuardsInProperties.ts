//@target: es5, es2015

// Note that type guards affect types of variables and parameters only and 
// have no effect on members of objects such as properties. 

let num: number = null as unknown as (number);
let strOrNum: string | number = null as unknown as (string | number);
class C1 {
    private pp1: string | number;
    pp2: string | number;
    // Inside public accessor getter
    get pp3() {
        return strOrNum;
    }
    method(): void {
        strOrNum = typeof this.pp1 === "string" && this.pp1; // string | number
        strOrNum = typeof this.pp2 === "string" && this.pp2; // string | number
        strOrNum = typeof this.pp3 === "string" && this.pp3; // string | number
    }
}
/*pruned*/;                          
/*pruned*/;                                      // string | number
/*pruned*/;                                      // string | number
let obj1: {
    x: string | number;
} = null as unknown as ({
    x: string | number;
});
strOrNum = typeof obj1.x === "string" && obj1.x;  // string | number

function main(): void {}
