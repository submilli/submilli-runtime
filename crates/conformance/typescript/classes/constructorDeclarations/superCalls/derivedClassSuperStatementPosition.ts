// @target: ES5, ES2015

class DerivedBasic extends Object {
    prop: number = 1;
    constructor() {
        super();
    }
}

class DerivedAfterParameterDefault extends Object {
    x1: boolean;
    x2: boolean;
    constructor(x: boolean = false) {
        this.x1 = x;
        super(x);
        this.x2 = x;
    }
}

class DerivedAfterRestParameter extends Object {
    x1: boolean[];
    x2: boolean[];
    constructor(...x: boolean[]) {
        this.x1 = x;
        super(x);
        this.x2 = x;
    }
}

/*pruned*/;                           
           
                   
             
                            
             
                      
             
                            
             
     
 

/*pruned*/;                                      
           
                   
             
             
             
                            
             
                      
             
                            
             
     
 

class DerivedInConditional extends Object {
    prop: number = 1;
    constructor() {
        Math.random()
            ? super(1)
            : super(0);
    }
}

class DerivedInIf extends Object {
    prop: number = 1;
    constructor() {
        if (Math.random()) {
            super(1);
        }
        else {
            super(0);
        }
    }
}

class DerivedInBlockWithProperties extends Object {
    prop: number = 1;
    constructor(private paramProp: number = 2) {
        {
            super();
        }
    }
}

class DerivedInConditionalWithProperties extends Object {
    prop: number = 1;
    constructor(private paramProp: number = 2) {
        if (Math.random()) {
            super(1);
        } else {
            super(0);
        }
    }
}


function main(): void {}
