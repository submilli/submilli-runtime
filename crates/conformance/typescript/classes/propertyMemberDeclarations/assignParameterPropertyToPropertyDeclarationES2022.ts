// @useDefineForClassFields: true
// @target: es2022
class C {
    qux: string = this.bar // should error
    bar: string = this.foo // should error
    quiz: string = this.bar // ok
    quench: void = this.m1() // ok
    quanch: void = this.m3() // should error
    m1(): void {
        this.foo // ok
    }
    m3: () => void = function() { }
    constructor(public foo: string) {}
    quim: string = this.baz // should error
    baz: string = this.foo; // should error
    quid: string = this.baz // ok
    m2(): void {
        this.foo // ok
    }
}

class D extends C {
    quill: string = this.foo // ok
}

class E {
    bar: () => string = () => this.foo1 + this.foo2; // both ok
    foo1: string = '';
    constructor(public foo2: string) {}
}

/**/;    
                             
                            
     
                  
 
/**/;    
                             
                            
     
                                     
 
class H {
    constructor(public p1: C) {}

    public p2: () => string = () => {
        return this.p1.foo;
    }

    public p3: () => string = () => this.p1.foo;
}


function main(): void {}
