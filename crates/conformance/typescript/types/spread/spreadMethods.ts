// @target: esnext
// @useDefineForClassFields: false

class K {
    p: number = 12;
    m(): void { }
    get g(): number { return 0; }
}
interface I {
    p: number;
    m(): void;
    readonly g: number;
}

let k = new K()
let sk = { ...k };
let ssk = { ...k, ...k };
sk.p;
sk.m(); // error
sk.g; // error
ssk.p;
ssk.m(); // error
ssk.g; // error

/*pruned*/;                                          
/*pruned*/;       
/*pruned*/;              
/**/;
/**/;   // ok
/**/; // ok
/**/; 
/**/;    // ok
/**/;  // ok

/*pruned*/;                                       
/*pruned*/;       
/*pruned*/;              
/**/;
/**/;   // ok
/**/; // ok
/**/; 
/**/;    // ok
/**/;  // ok


function main(): void {}
