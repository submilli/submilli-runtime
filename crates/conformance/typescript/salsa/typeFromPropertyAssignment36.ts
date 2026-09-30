// @target: es2015
// @strict: true
/*pruned*/;                                                                         
                        
     
            
       

            
                   
     
                                      
       
            
                   
     
          
                  
     
       
            
               
     
          
               
     
       
            
                  
     
            
 
// OK to access possibly-unassigned properties outside the initialising scope
/*pruned*/;         

function d(): void {
}
d.e = 12
d.e

if (!!false) {
    d.q = false
}
d.q
if (!!false) {
    d.q = false
}
else {
    d.q = true
}
d.q
if (!!false) {
    d.r = 1
}
else {
    d.r = 2
}
d.r

// test function expressions too
const g = function() {
}
if (!!false) {
    g.expando = 1
}
g.expando // error

if (!!false) {
    g.both = 'hi'
}
else {
    g.both = 0
}
g.both


function main(): void {}
