// @target: es2015
const a = 'a';
const b = 'b';
const d = 'd';

/*pruned*/;               
/*pruned*/;               

/*pruned*/;                                 

/*pruned*/;    
                
                      
 

/*pruned*/;    
               
 

/*pruned*/;  
              
                    
 

/*pruned*/;  
               
 

// repro from https://github.com/microsoft/TypeScript/issues/54790

/*pruned*/;             
                         
                                    
           
                         
             
   
                         
              
   
                                              
                               
                              
                   
     
   
                                            
 

function uniqueID_54790_2(id: string | number, seenIDs: object): void {
  id = "a";
  for (let i = 1; i < 3; i++) {
    const newID = `${id}`;
    if (newID in seenIDs) {
    }
  }
}

function uniqueID_54790_3(id: string | number, seenIDs: object): void {
  id = "a";
  for (let i = 1; i < 3; i++) {
    const newID = id;
    if (newID in seenIDs) {
    }
  }
}


function main(): void {}
