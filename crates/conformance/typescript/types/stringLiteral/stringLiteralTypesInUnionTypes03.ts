// @target: es2015
// @strict: true
// @declaration: true

type T = number | "foo" | "bar";

let x: "foo" | "bar" | number = null as unknown as ("foo" | "bar" | number);
/*pruned*/;          

if (x === "foo") {
    let a = x;
}
else if (x !== "bar") {
    /*pruned*/;    
}
else {
    let c = x;
    /**/;     
    /*pruned*/;                             
}

/**/; 
/**/; 

function main(): void {}
