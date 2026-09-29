// @target: es2015
// type of 'this' in FunctionExpression is Any

function fn(): void {
    let p = this;
    /*pruned*/;                             
}

let t = function () {
    let p = this;
    /*pruned*/;                             
}

let t2 = function f() {
    let x = this;
    /*pruned*/;                             
}

class C {
    x: () => void = function () {
        /*pruned*/;                           
        let q_2 = this;
    }
    y: () => void = function ff() {
        /*pruned*/;                           
        let q_2 = this;
    }
}

/*pruned*/;  
                         
                     
                                                
     

                         
                     
                                                
     

                           
                     
                                                
     

 

function main(): void {}
