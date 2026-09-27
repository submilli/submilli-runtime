// @module: commonjs
// @target: es2015
/*pruned*/;     
                        
                      
     
 

/*pruned*/;                           
                
 

class X {
    static now(): {} {
        return {}
    }

    why(): void {

    }
}

class Y {

}

console.log(X.now()) // works as expected
/*pruned*/;          // works as expected

export const x: X | number = Math.random() > 0.5 ? new X() : 1

if (x instanceof X) {
    x.why() // should compile
}

function main(): void {}
