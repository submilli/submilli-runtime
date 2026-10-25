// @target: es2015
// @strict: true
const [a, b = a] = [1]; // ok
/*pruned*/;                    // error for e = e
const [f, g = f, h = i, i = f] = [1]; // error for h = i

(function ([a, b = a]) { // ok
})([1]);
/*pruned*/;                                       
        
(function ([f, g = f, h = i, i = f]) { // error for h = i
})([1])


function main(): void {}
