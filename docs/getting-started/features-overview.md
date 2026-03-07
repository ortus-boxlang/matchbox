# Features Overview

MatchBox supports the core BoxLang language. This page covers each major feature group with brief examples. For a full language reference, see the [BoxLang documentation](https://boxlang.io/docs).

---

## Variables & Types

Variables are dynamically typed and can be assigned without declaration.

```boxlang
name   = "Jacob"          // String
age    = 30               // Number
active = true             // Boolean
score  = 9.5              // Number (float)
```

Use `var` for block-scoped variables inside functions:

```boxlang
function example() {
    var count = 0   // scoped to this function
    count = count + 1
    return count
}
```

---

## Operators

| Category | Operators |
| :--- | :--- |
| Arithmetic | `+`, `-`, `*`, `/`, `%` |
| Comparison | `==`, `!=`, `<`, `>`, `<=`, `>=` |
| Logical | `&&`, `\|\|`, `!` |
| String concat | `&` |
| Increment / Decrement | `++`, `--` |

```boxlang
result = (10 + 5) * 2          // 30
greeting = "Hello" & ", " & name  // String concatenation
```

---

## Strings & Interpolation

MatchBox supports BoxLang-style `#variable#` interpolation inside double-quoted strings:

```boxlang
city = "Austin"
println("Welcome to #city#!")         // Welcome to Austin!
println("1 + 1 = #(1 + 1)#")         // 1 + 1 = 2
```

---

## Control Flow

### If / Else

```boxlang
score = 85

if (score >= 90) {
    println("A")
} else if (score >= 80) {
    println("B")
} else {
    println("C or below")
}
```

### For Loop

```boxlang
for (i = 1; i <= 5; i++) {
    println(i)
}
```

### For-In Loop

```boxlang
colors = ["red", "green", "blue"]

for (color in colors) {
    println(color)
}
```

For-in also works on structs, iterating over keys:

```boxlang
user = { name: "Jacob", age: 30 }

for (key in user) {
    println(key & " = " & user[key])
}
```

---

## Functions

### Basic Functions

```boxlang
function add(a, b) {
    return a + b
}

println(add(3, 4))   // 7
```

### Default Arguments

```boxlang
function greet(name = "World", greeting = "Hello") {
    return greeting & ", " & name & "!"
}

println(greet())            // Hello, World!
println(greet("Jacob"))     // Hello, Jacob!
```

### Required Arguments & Type Hints

```boxlang
public string function greet(required string name) {
    return "Hello, " & name
}

private numeric function add(numeric a, numeric b) {
    return a + b
}
```

### Arrow Functions (Lambdas)

```boxlang
double   = (x)    => x * 2
add      = (x, y) => x + y
sayHello = ()     => println("Hello!")
```

### Closures

```boxlang
function makeCounter() {
    count = 0
    return () => {
        count = count + 1
        return count
    }
}

counter = makeCounter()
println(counter())   // 1
println(counter())   // 2
```

---

## Arrays

Arrays are 1-indexed and can contain mixed types.

```boxlang
fruits = ["apple", "banana", "cherry"]

println(fruits[1])              // apple
arrayAppend(fruits, "date")
println(arrayLen(fruits))       // 4

// Iteration
for (fruit in fruits) {
    println(fruit)
}
```

### Common Array BIFs

| BIF | Description |
| :--- | :--- |
| `arrayLen(arr)` | Number of elements |
| `arrayAppend(arr, val)` | Append a value |
| `arrayMap(arr, fn)` | Map over elements |
| `arrayToList(arr, delim)` | Join as delimited string |

---

## Structs

Structs are key-value maps. Keys are case-insensitive.

```boxlang
user = {
    name: "Jacob",
    age:  30,
    active: true
}

println(user.name)      // Jacob
println(user["AGE"])    // 30 (case-insensitive)

user.email = "jacob@example.com"   // add a key
```

---

## Classes & Objects

### Defining a Class

```boxlang
class Person {
    property name
    property age

    function init(name, age) {
        this.name = name
        this.age  = age
        return this
    }

    function greet() {
        return "Hi, I'm " & this.name
    }
}

p = new Person()
p.init("Jacob", 30)
println(p.greet())    // Hi, I'm Jacob
```

### Implicit Accessors

When `accessors="true"` is set on the class, MatchBox auto-generates `getX()` and `setX()` methods for every `property`:

```boxlang
class Product accessors="true" {
    property name
    property price
}

p = new Product()
p.setName("Widget")
p.setPrice(9.99)
println(p.getName())   // Widget
```

### Inheritance

```boxlang
class Animal {
    function speak() { return "..." }
}

class Dog extends="Animal" {
    function speak() { return "Woof!" }
}

d = new Dog()
println(d.speak())    // Woof!
```

### onMissingMethod

Intercept calls to methods that don't exist, enabling dynamic dispatch patterns:

```boxlang
class DynamicModel {
    function onMissingMethod(methodName, args) {
        println("Called: " & methodName)
        return "dynamic_result"
    }
}

model = new DynamicModel()
model.findUserByName("Jacob")   // Called: findUserByName
```

---

## Interfaces

Interfaces define a contract. A class that `implements` an interface must provide all of its abstract methods.

```boxlang
interface IGreetable {
    function greet(name);
}

class Person implements="IGreetable" {
    function greet(name) {
        println("Hello, " & name)
    }
}
```

Interfaces can also provide default method implementations (trait-like behaviour):

```boxlang
interface ISpeakable {
    function speak() {
        println("I can speak!")
    }
}

class Person implements="ISpeakable" {
    // speak() inherited from the interface default
}
```

---

## Exception Handling

```boxlang
try {
    throw "Something went wrong!"
} catch (e) {
    println("Caught: " & e)
} finally {
    println("Always runs")
}
```

Division by zero automatically throws:

```boxlang
try {
    result = 10 / 0
} catch (e) {
    println("Error: " & e)
}
```

---

## Async / Concurrency

MatchBox includes a cooperative fiber scheduler. Use `runAsync` to execute work concurrently:

```boxlang
future = runAsync(() => {
    sleep(100)
    return "done"
})

result = future.get()
println(result)   // done
```

Chain `.onError()` for async error handling:

```boxlang
future = runAsync(() => {
    throw "async error"
})
.onError((e) => println("Caught async error: " & e))
```

---

## JavaScript Interop (WASM only)

When running in the browser via WASM, the `js` global object provides access to the JavaScript environment:

```boxlang
// Read from the DOM
href = js.window.location.href
println("Current page: " & href)

// Show an alert
js.alert("Hello from BoxLang!")

// Access browser APIs
js.console.log("Logged from BoxLang")
```

> **Note:** `js.*` APIs are only available when MatchBox is running inside a browser or JS runtime. They will throw an error in native builds.

---

## Native Fusion (Rust Interop)

Native Fusion lets you write performance-critical code in Rust and call it directly from BoxLang. See [Native Builds](../building-and-deploying/native-builds.md#native-fusion-rust-interop) for the complete guide.

```boxlang
// Calls a Rust function bundled into the binary
result = fast_process(data)
println("Result: " & result)
```

---

## Standard Library (Prelude)

MatchBox includes a small standard library of BIFs compiled into every program. A selection:

| Function | Description |
| :--- | :--- |
| `println(...)` | Print with newline |
| `arrayLen(arr)` | Length of an array |
| `arrayAppend(arr, val)` | Append to an array |
| `arrayMap(arr, fn)` | Map over an array |
| `arrayToList(arr, delim)` | Join array as string |
| `abs(n)` | Absolute value |
| `min(a, b)` | Minimum of two numbers |
| `max(a, b)` | Maximum of two numbers |
| `sleep(ms)` | Pause for `ms` milliseconds |
| `runAsync(fn)` | Run function on the fiber scheduler |
