// @target: es2015
// all of these valueOf calls should return the type shown in the overriding signatures here

class C {
    valueOf(): void { }
}

/*pruned*/;                       
/*pruned*/;                

interface I {
    valueOf(): void;
}

/*pruned*/;                       
/*pruned*/;                

let a = {
    valueOf: () => { }
}

let r3: void = a.valueOf();

let b: {
    valueOf(): void;
} = null as unknown as ({
    valueOf(): void;
});

let r4: void = b.valueOf();

function main(): void {}
