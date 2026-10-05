// @target: es2015
let x: {
    foo: string,
    bar: string
} = null as unknown as ({
    foo: string,
    bar: string
});

// ASI makes this work
let y: {
    foo: string
    bar: string
} = null as unknown as ({
    foo: string
    bar: string
});

/*pruned*/;                                                                           

function main(): void {}
