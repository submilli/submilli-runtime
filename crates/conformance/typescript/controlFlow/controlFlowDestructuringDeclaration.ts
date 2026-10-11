// @target: es2015
// @strictNullChecks: true

function f1(): void {
    let x: string | number = 1;
    x;
    let y: string | undefined = "";
    y;
}

function f2(): void {
    let [x]: [string | number] = [1];
    x;
    let [y]: [string | undefined] = [""];
    y;
    let [z = ""]: [string | undefined] = [undefined];
    z;
}

function f3(): void {
    let [x]: (string | number)[] = [1];
    x;
    let [y]: (string | undefined)[] = [""];
    y;
    let [z = ""]: (string | undefined)[] = [undefined];
    z;
}

function f4(): void {
    let { x }: { x: string | number } = { x: 1 };
    x;
    let { y }: { y: string | undefined } = { y: "" };
    y;
    let { z = "" }: { z: string | undefined } = { z: undefined };
    z;
}

function f5(): void {
    let { x }: { x?: string | number } = { x: 1 };
    x;
    let { y }: { y?: string | undefined } = { y: "" };
    y;
    let { z = "" }: { z?: string | undefined } = { z: undefined };
    z;
}

function f6(): void {
    let { x }: { x?: string | number } = {};
    x;
    let { y }: { y?: string | undefined } = {};
    y;
    let { z = "" }: { z?: string | undefined } = {};
    z;
}

/*pruned*/;          
                                              
                                                    
      
 


function main(): void {}
