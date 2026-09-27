// @target: es2015
let a: {} = null as unknown as ({});
/*pruned*/;                                 

function foo<T, U/* extends T*/, V/* extends U*/>(t: T, u: U, v: V): void {
    // errors
    let ra1 = t < u;
    let ra2 = t > u;
    let ra3 = t <= u;
    let ra4 = t >= u;
    let ra5 = t == u;
    let ra6 = t != u;
    let ra7 = t === u;
    let ra8 = t !== u;

    let rb1 = u < t;
    let rb2 = u > t;
    let rb3 = u <= t;
    let rb4 = u >= t;
    let rb5 = u == t;
    let rb6 = u != t;
    let rb7 = u === t;
    let rb8 = u !== t;

    let rc1 = t < v;
    let rc2 = t > v;
    let rc3 = t <= v;
    let rc4 = t >= v;
    let rc5 = t == v;
    let rc6 = t != v;
    let rc7 = t === v;
    let rc8 = t !== v;

    let rd1 = v < t;
    let rd2 = v > t;
    let rd3 = v <= t;
    let rd4 = v >= t;
    let rd5 = v == t;
    let rd6 = v != t;
    let rd7 = v === t;
    let rd8 = v !== t;

    // ok
    let re1 = t < a;
    let re2 = t > a;
    let re3 = t <= a;
    let re4 = t >= a;
    let re5 = t == a;
    let re6 = t != a;
    let re7 = t === a;
    let re8 = t !== a;

    let rf1 = a < t;
    let rf2 = a > t;
    let rf3 = a <= t;
    let rf4 = a >= t;
    let rf5 = a == t;
    let rf6 = a != t;
    let rf7 = a === t;
    let rf8 = a !== t;

    /*pruned*/;     
    /*pruned*/;     
    /*pruned*/;      
    /*pruned*/;      
    /*pruned*/;      
    /*pruned*/;      
    /*pruned*/;       
    /*pruned*/;       

    /*pruned*/;     
    /*pruned*/;     
    /*pruned*/;      
    /*pruned*/;      
    /*pruned*/;      
    /*pruned*/;      
    /*pruned*/;       
    /*pruned*/;       
}

function main(): void {}
