// @target: es2015
// Note that type guards affect types of variables and parameters only and 
// have no effect on members of objects such as properties. 

// Note that the class's property must be copied to a local variable for
// the type guard to have an effect
class D {
    data: string | string[];
    getData(): string {
        let data = this.data;
        return typeof data === "string" ? data : data.join(" ");
    }

    getData1(): string {
        return typeof this.data === "string" ? this.data : this.data.join(" ");
    }
}

let o: {
    prop1: number|string;
    prop2: boolean|string;
} = {
        prop1: "string" ,
        prop2: true
    }

if (typeof o.prop1 === "string" && o.prop1.toLowerCase()) {}
let prop1 = o.prop1;
if (typeof prop1 === "string" && prop1.toLocaleLowerCase()) { }

function main(): void {}
