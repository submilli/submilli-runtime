// @target: es2015
// @strict: true
// @declaration: true

type PrimitiveName = 'string' | 'number' | 'boolean';

/*pruned*/;                                     
/*pruned*/;                                     
/*pruned*/;                                       
/*pruned*/;                                                           
/*pruned*/;                                                           
/*pruned*/;                                                         
/*pruned*/;                                                                               
function getFalsyPrimitive(x: PrimitiveName): number | string | boolean {
    if (x === "string") {
        return "";
    }
    if (x === "number") {
        return 0;
    }
    if (x === "boolean") {
        return false;
    }

    // Should be unreachable.
    throw "Invalid value";
}

/*pruned*/;        
                                                     
                                             
                                               
 

const string: "string" = "string"
const number: "number" = "number"
const boolean: "boolean" = "boolean"

const stringOrNumber = string || number;
const stringOrBoolean = string || boolean;
const booleanOrNumber = number || boolean;
const stringOrBooleanOrNumber = stringOrBoolean || number;

/*pruned*/;        
                                                   
                                           
                                             

                                                
                                                 
                                                 
                                                         
 




function main(): void {}
