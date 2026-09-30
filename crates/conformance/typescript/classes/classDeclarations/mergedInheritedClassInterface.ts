// @target: es2015
interface BaseInterface {
    required: number;
    optional?: number;
}

class BaseClass {
    baseMethod(): void { }
    baseNumber: number;
}

interface Child extends BaseInterface {
    additional: number;
}

class Child extends BaseClass {
    classNumber: number;
    method(): void { }
}

interface ChildNoBaseClass extends BaseInterface {
    additional2: string;
}
class ChildNoBaseClass {
    classString: string;
    method2(): void { }
}
class Grandchild extends ChildNoBaseClass {
}

// checks if properties actually were merged
let child : Child = null as unknown as (Child);
child.required;
child.optional;
child.additional;
child.baseNumber;
child.classNumber;
child.baseMethod();
child.method();

/*pruned*/;                                                  
/*pruned*/;         
/*pruned*/;         
/*pruned*/;            
/*pruned*/;            
/*pruned*/;          


function main(): void {}
