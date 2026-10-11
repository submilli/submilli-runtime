// @strict: true
// @target: esnext, es2021, es2020, es2015
interface A {
    foo: {
        bar(): {
            baz: 0 | 1 | 42 | undefined | ''
        }
        baz: 0 | 1 | 42 | undefined | ''
    }
    baz: 0 | 1 | 42 | undefined | ''
}

const result: A = null as unknown as (A);
const a: A = null as unknown as (A);
const b: A = null as unknown as (A);
const c: A = null as unknown as (A);

/*pruned*/;         
/*pruned*/;         
/*pruned*/;         

/*pruned*/;                    
/*pruned*/;                    
/*pruned*/;                    

/*pruned*/;                             
/*pruned*/;                             
/*pruned*/;                             



function main(): void {}
