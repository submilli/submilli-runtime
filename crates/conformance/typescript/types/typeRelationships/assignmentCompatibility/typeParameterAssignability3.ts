// @target: es2015
// type parameters are not assignable to one another unless directly or indirectly constrained to one another

class Foo { foo: string; }

function foo<T extends Foo, U extends Foo>(t: T, u: U): void {
    /*pruned*/;                       
    /*pruned*/;                       
    /**/;  // ok
    /**/;  // ok

    /**/;  // ok
    /**/;  // ok

    t = u; // error
    u = t; // error
}

class C<T extends Foo, U extends Foo> {
    t: T;
    u: U;
    r: () => void = () => {
        this.t = this.u; // error
        this.u = this.t; // error
    }
}

function main(): void {}
