// @target: es2015
// @declaration: true

class BaseA {
    public constructor(public x: number) { }
    createInstance(): void { new BaseA(1); }
}

/*pruned*/;  
                                               
                                            
 

class BaseC {
    private constructor(public x: number) { }
    createInstance(): void { new BaseC(3); }
    static staticInstance(): void { new BaseC(4); }
}

class DerivedA extends BaseA {
    constructor(public x: number) { super(x); }
    createInstance(): void { new DerivedA(5); }
    createBaseInstance(): void { new BaseA(6); }
    static staticBaseInstance(): void { new BaseA(7); }
}

/*pruned*/;                   
                                               
                                               
                                                      
                                                             
 

class DerivedC extends BaseC { // error
    constructor(public x: number) { super(x); }
    createInstance(): void { new DerivedC(9); }
    createBaseInstance(): void { new BaseC(10); } // error
    static staticBaseInstance(): void { new BaseC(11); } // error
}

let ba = new BaseA(1);
/*pruned*/;            // error
let bc = new BaseC(1); // error

let da = new DerivedA(1);
/*pruned*/;              
let dc = new DerivedC(1);


function main(): void {}
