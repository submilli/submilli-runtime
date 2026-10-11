// @target: es2015
class Base {
    x: number = 43;
    constructor(n: string) {

    }
}

function v(): void { }

class Derived extends Base {
    //super call in class constructor of derived type
    constructor(public q: number) {
        super('');
        //type of super call expression is void
        let p = super('');
        let p_2 = v();
    }
}

class OtherBase {

}

class OtherDerived extends OtherBase {
    constructor() {
        let p = '';
        super();
    }
}


function main(): void {}
