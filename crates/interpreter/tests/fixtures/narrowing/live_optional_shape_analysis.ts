function read(values: {value:number}[] | null):void { assert(values?.[0].value === 3); }
function main():void { read([{value:3}]); }
