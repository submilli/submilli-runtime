// @target: es2015
// @strict: true
// @allowUnreachableCode: true

/*pruned*/;           
            
 

/*pruned*/;              

let y: string = null as unknown as (string);
switch (y) {
    case 'a':
        throw y;
    default:
        throw y;
}

let z = 0;
while (z < 10) {
    throw z;
}

for (let i = 0; ;) { throw i; }

/*pruned*/;                       

do { throw null; }while(true)

let j = 0;
while (j < 0) { throw j; }

class C<T> {
    private value: T;
    biz(): void {
        throw this.value;
    }

    constructor() {
        throw this;
    }
}

let aa = {
    id:12,
    biz() {
        throw this;
    }
}


function main(): void {}
