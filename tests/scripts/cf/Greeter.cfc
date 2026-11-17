// A CF component (issue #45): `component` is read as `class`.
component {
    function init(name) {
        this.name = name;
        return this;
    }

    function greet() {
        return "Hello, " & this.name & "!";
    }
}
