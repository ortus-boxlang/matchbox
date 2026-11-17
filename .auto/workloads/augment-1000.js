function add(a,b) { return a+b; }
function batch(n) {
    var sum=0;
    for (var i=0; i<n; i++) { var x=i; x+=2; sum+=x; }
    return sum;
}
console.log("PRIMED");
for (var w=0; w<100; w++) { batch(1000); Atomics.wait(new Int32Array(new SharedArrayBuffer(4)),0,0,1); }
console.log("__ICOUNT_START__");
var result=batch(1000);
console.log("__ICOUNT_END__");
console.log(result);
