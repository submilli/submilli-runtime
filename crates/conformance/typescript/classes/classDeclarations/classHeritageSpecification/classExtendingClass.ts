// @target: es2015
class C {
    foo: string;
    thing(): void { }
    static other(): void { }
}

class D extends C {
    bar: string;
}

/*pruned*/;                       
/*pruned*/;   
/*pruned*/;    
/*pruned*/;        
let r4 = D.other();

class C2<T> {
    foo: T;
    thing(x: T): void { }
    static other<T>(x: T): void { }
}

class D2<T> extends C2<T> {
    bar: string;
}

/*pruned*/;                                          
/*pruned*/;     
/*pruned*/;     
/*pruned*/;           
let r8 = D2.other(1);

function main(): void {}
