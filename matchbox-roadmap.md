
# MatchBox: Micro BoxLang Runtime for Native Deployment, Embedded Devices, and WASM

Should MatchBox be just a VM that the core BoxLang runtime can use as a compilation target or should it handle compilation itself?

# Language Subset

## Included

Classes 
    extends         ✅
    accessors       ✅
    properties      ✅
    static
    final?
    onMissingMethod ✅
interfaces          ✅
    default methods ✅
functions
    UDFs            ✅
    closures        ✅
    lambdas         ✅
BIFS
    async
    array
    struct
    cli
    file
    query?
    date
type annotations    ✅
control flow        ✅
    if/else         ✅
    while           ✅
    for in          ✅
    for index       ✅
elvis               ✅
safe navigation     ✅
ternary             ✅
scripts             ✅
classes             ✅
string interpolation✅
try/catch/finally   ✅
switch statement?   ✅   
runAsync            ✅
javaInterop         ✅
rustInterop         ✅
imports             ✅
includes?           
named arguments     ✅
argumentCollection  
destructuring

## Undecided


match operator
Metadata
queryOfQueries?
configuration
modules
scheduler
cache
Application.bx/application scope
interceptors?
change listeners?


## Excluded

abstract
components
templates
scope lookup
    request scope
    cgi scope
    server scope

# Compilation

Option 1: Matchbox standalone
Option 2: Matchbox is VM only, add new compiler through bx-matchbox module

# BoxLang Modules

Add support in compilers for annotations to enable/disable a function for certain compilers

```
// usually not needed as it is the default
@boxpiler( "asm" )
function doStuffWithJava(){
    var obj = new java.lang.String( "what" );
}

@boxpiler( "matchbox" )
function doStuffNatively(){
    // calls to rust
}
```

In the future enhance the module spec to provide metadata if a package supports matchbox.

## Runtime Framework Features: Datasources and Other Things

Right now we have several language concepts that are implemneted in Java and cannot easily be implemented in pure BoxLang.

Do we want to just make this a strong separation of concerns or do we want to create BoxLang framework interfaces so that these concepts can be expressed in pure BoxLang? This would make it much easier for us to write the library/module once and share it across runtimes.

# More Ideas

Make BIFs excludeable from runtime
Add a --production flag that will remove debug info and tree-shake
Can the BIFs be implemented in BoxLang?
    If we implment things as BIFs they can be registered after the vm code and before the app code so we don't have to cross compile them at the rust level and can exclude them when compiling a BoxLang app