// @target: es2015
// @noImplicitAny: true
// @noImplicitThis: true

class MyClass {
    t: number;

    fn(): void {
        /*pruned*/;                
        //type of 'this' in an object literal is the containing scope's this
        let t = { x: this, y: this.t };
        /*pruned*/;                                                                                       
    }
}

//type of 'this' in an object literal method is the type of the object literal
let obj = {
    f() {
        return this.spaaace;
    }
};
/*pruned*/;                                                           


function main(): void {}
