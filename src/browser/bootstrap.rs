use matchbox_compiler::ast;

pub fn exported_function_names(ast: &[ast::Statement]) -> Vec<String> {
    let mut functions = Vec::new();
    for stmt in ast {
        if let ast::StatementKind::FunctionDecl { name, .. } = &stmt.kind {
            functions.push(name.clone());
        }
    }
    functions
}

pub fn render_pure_js_bootstrap(functions: &[String], b64_wasm: &str, b64_bytecode: &str) -> String {
    let mut bootstrap = String::new();
    bootstrap.push_str(&format!("const wasmBase64 = \"{}\";\n", b64_wasm));
    bootstrap.push_str(&format!("const bytecodeBase64 = \"{}\";\n\n", b64_bytecode));

    bootstrap.push_str("let vm = null;\n");
    bootstrap.push_str("async function ensureInit() {\n");
    bootstrap.push_str("    if (vm) return;\n");
    bootstrap.push_str("    const wasmBinary = Uint8Array.from(atob(wasmBase64), c => c.charCodeAt(0));\n");
    bootstrap.push_str("    await init(wasmBinary);\n");
    bootstrap.push_str("    vm = new BoxLangVM();\n");
    bootstrap.push_str("    const bytecodeBinary = Uint8Array.from(atob(bytecodeBase64), c => c.charCodeAt(0));\n");
    bootstrap.push_str("    vm.load_bytecode(bytecodeBinary);\n");
    bootstrap.push_str("}\n\n");

    for func in functions {
        bootstrap.push_str(&format!("export async function {}(...args) {{\n", func));
        bootstrap.push_str("    await ensureInit();\n");
        bootstrap.push_str(&format!("    try {{\n"));
        bootstrap.push_str(&format!("        return await vm.call(\"{}\", args);\n", func));
        bootstrap.push_str(&format!("    }} catch (e) {{\n"));
        bootstrap.push_str(&format!("        if (e instanceof Error) throw e;\n"));
        bootstrap.push_str(&format!("        throw new Error(String(e));\n"));
        bootstrap.push_str(&format!("    }}\n"));
        bootstrap.push_str("}\n\n");
    }

    bootstrap
}

pub fn render_fusion_js_bootstrap(functions: &[String], module_name: &str) -> String {
    let mut bootstrap = String::new();
    bootstrap.push_str("if (typeof window !== \"undefined\") {\n");
    bootstrap.push_str("    window.MatchBox = window.MatchBox || {};\n");
    bootstrap.push_str("    window.MatchBox._callbackBridges = window.MatchBox._callbackBridges || new Map();\n");
    bootstrap.push_str("    window.MatchBox._pumpBridges = window.MatchBox._pumpBridges || new Map();\n");
    bootstrap.push_str("    window.MatchBox.registerCallbackBridge = window.MatchBox.registerCallbackBridge || function(vmPtr, bridge) {\n");
    bootstrap.push_str("        window.MatchBox._callbackBridges.set(vmPtr, bridge);\n");
    bootstrap.push_str("    };\n");
    bootstrap.push_str("    window.MatchBox.registerPumpBridge = window.MatchBox.registerPumpBridge || function(vmPtr, bridge) {\n");
    bootstrap.push_str("        window.MatchBox._pumpBridges.set(vmPtr, bridge);\n");
    bootstrap.push_str("    };\n");
    bootstrap.push_str("    window.MatchBox.unregisterCallbackBridge = window.MatchBox.unregisterCallbackBridge || function(vmPtr) {\n");
    bootstrap.push_str("        window.MatchBox._callbackBridges.delete(vmPtr);\n");
    bootstrap.push_str("        window.MatchBox._pumpBridges.delete(vmPtr);\n");
    bootstrap.push_str("    };\n");
    bootstrap.push_str("    window.MatchBox.invokeCallback = window.MatchBox.invokeCallback || function(vmPtr, callbackId, thisVal, args) {\n");
    bootstrap.push_str("        const bridge = window.MatchBox._callbackBridges.get(vmPtr);\n");
    bootstrap.push_str("        if (!bridge) {\n");
    bootstrap.push_str("            throw new Error(\"MatchBox callback bridge is not registered for VM \" + vmPtr);\n");
    bootstrap.push_str("        }\n");
    bootstrap.push_str("        return bridge(vmPtr, callbackId, thisVal, args);\n");
    bootstrap.push_str("    };\n");
    bootstrap.push_str("    window.MatchBox.schedulePump = window.MatchBox.schedulePump || function(vmPtr) {\n");
    bootstrap.push_str("        const bridge = window.MatchBox._pumpBridges.get(vmPtr);\n");
    bootstrap.push_str("        if (!bridge) {\n");
    bootstrap.push_str("            return;\n");
    bootstrap.push_str("        }\n");
    bootstrap.push_str("        const pump = () => {\n");
    bootstrap.push_str("            const bridge = window.MatchBox._pumpBridges.get(vmPtr);\n");
    bootstrap.push_str("            if (bridge) {\n");
    bootstrap.push_str("                try {\n");
    bootstrap.push_str("                    bridge();\n");
    bootstrap.push_str("                } catch (error) {\n");
    bootstrap.push_str("                    if (typeof console !== \"undefined\" && console.error) {\n");
    bootstrap.push_str("                        console.error(\"MatchBox scheduled pump failed\", error);\n");
    bootstrap.push_str("                    }\n");
    bootstrap.push_str("                }\n");
    bootstrap.push_str("            }\n");
    bootstrap.push_str("        };\n");
    bootstrap.push_str("        if (typeof queueMicrotask === \"function\") {\n");
    bootstrap.push_str("            queueMicrotask(pump);\n");
    bootstrap.push_str("        } else {\n");
    bootstrap.push_str("            setTimeout(pump, 0);\n");
    bootstrap.push_str("        }\n");
    bootstrap.push_str("    };\n");
    bootstrap.push_str("    window.MatchBox.getInstanceProperty = window.MatchBox.getInstanceProperty || function(vmPtr, gcId, name) {\n");
    bootstrap.push_str("        if (typeof _matchbox_get_instance_prop !== \"function\") return undefined;\n");
    bootstrap.push_str("        return _matchbox_get_instance_prop(vmPtr, gcId, name);\n");
    bootstrap.push_str("    };\n");
    bootstrap.push_str("    window.MatchBox.getInstanceKeys = window.MatchBox.getInstanceKeys || function(vmPtr, gcId) {\n");
    bootstrap.push_str("        if (typeof _matchbox_get_instance_keys !== \"function\") return [];\n");
    bootstrap.push_str("        return _matchbox_get_instance_keys(vmPtr, gcId);\n");
    bootstrap.push_str("    };\n");
    bootstrap.push_str("    window.MatchBox.setInstanceProperty = window.MatchBox.setInstanceProperty || function(vmPtr, gcId, name, value) {\n");
    bootstrap.push_str("        if (typeof _matchbox_set_instance_prop !== \"function\") return;\n");
    bootstrap.push_str("        _matchbox_set_instance_prop(vmPtr, gcId, name, value);\n");
    bootstrap.push_str("    };\n");
    bootstrap.push_str(r#"    window.MatchBox.wrapInstancePropertyValue = window.MatchBox.wrapInstancePropertyValue || function(vmPtr, gcId, prop, value, ownerReceiver) {
        if (value == null || typeof value !== "object") return value;
        if (ArrayBuffer.isView(value) || value instanceof ArrayBuffer || value instanceof Date || value instanceof RegExp || value instanceof Promise) {
            return value;
        }
        if (value.__matchbox_nested_proxy__) return value;
        const persist = () => {
            if (ownerReceiver != null && typeof ownerReceiver === "object") {
                Reflect.set(ownerReceiver, prop, value, ownerReceiver);
            } else {
                window.MatchBox.setInstanceProperty(vmPtr, gcId, prop, value);
            }
        };
        const wrapChild = child => window.MatchBox.wrapInstancePropertyValue(vmPtr, gcId, prop, child, ownerReceiver);
        for (const key of Reflect.ownKeys(value)) {
            const current = value[key];
            if (current != null && typeof current === "object") {
                value[key] = wrapChild(current);
            }
        }
        return new Proxy(value, {
            get(target, nestedProp, receiver) {
                if (nestedProp === "__matchbox_nested_proxy__") return true;
                const nestedValue = Reflect.get(target, nestedProp, target);
                return nestedValue != null && typeof nestedValue === "object" ? wrapChild(nestedValue) : nestedValue;
            },
            set(target, nestedProp, nestedValue, receiver) {
                const wrapped = nestedValue != null && typeof nestedValue === "object" ? wrapChild(nestedValue) : nestedValue;
                const result = Reflect.set(target, nestedProp, wrapped, receiver);
                persist();
                return result;
            },
            deleteProperty(target, nestedProp) {
                const result = Reflect.deleteProperty(target, nestedProp);
                persist();
                return result;
            }
        });
    };
"#);
    bootstrap.push_str("    window.MatchBox.createInstanceProxy = window.MatchBox.createInstanceProxy || function(vmPtr, gcId) {\n");
    bootstrap.push_str("        const target = { __matchbox_vm_ptr: vmPtr, __matchbox_gc_id: gcId, __matchbox_cache: {} };\n");
    bootstrap.push_str("        return new Proxy(target, {\n");
    bootstrap.push_str("            get(target, prop, receiver) {\n");
    bootstrap.push_str("                if (typeof prop !== \"string\") return target[prop];\n");
    bootstrap.push_str("                if (prop.startsWith(\"__matchbox_\")) return target[prop];\n");
    bootstrap.push_str("                if (target.__matchbox_cache[prop]) return target.__matchbox_cache[prop];\n");
    bootstrap.push_str("                let val = window.MatchBox.getInstanceProperty(vmPtr, gcId, prop);\n");
    bootstrap.push_str("                if (val === undefined) {\n");
    bootstrap.push_str("                    const keys = window.MatchBox.getInstanceKeys(vmPtr, gcId);\n");
    bootstrap.push_str("                    const lowerProp = prop.toLowerCase();\n");
    bootstrap.push_str("                    const matchedKey = keys.find(k => k.toLowerCase() === lowerProp);\n");
    bootstrap.push_str("                    if (matchedKey && matchedKey !== prop) {\n");
    bootstrap.push_str("                        val = window.MatchBox.getInstanceProperty(vmPtr, gcId, matchedKey);\n");
    bootstrap.push_str("                    }\n");
    bootstrap.push_str("                }\n");
    bootstrap.push_str("                if (typeof val === \"function\") {\n");
    bootstrap.push_str("                    // Keep methods unbound so `this` stays on the actual JS receiver.\n");
    bootstrap.push_str("                    // That lets reactive wrappers observe BoxLang instance writes.\n");
    bootstrap.push_str("                    target.__matchbox_cache[prop] = val;\n");
    bootstrap.push_str("                } else if (val != null && typeof val === \"object\") {\n");
    bootstrap.push_str("                    val = window.MatchBox.wrapInstancePropertyValue(vmPtr, gcId, prop, val, receiver);\n");
    bootstrap.push_str("                }\n");
    bootstrap.push_str("                return val !== undefined ? val : target[prop];\n");
    bootstrap.push_str("            },\n");
    bootstrap.push_str("            set(target, prop, value) {\n");
    bootstrap.push_str("                if (typeof prop !== \"string\") return false;\n");
    bootstrap.push_str("                if (prop.startsWith(\"__matchbox_\")) { target[prop] = value; return true; }\n");
    bootstrap.push_str("                const keys = window.MatchBox.getInstanceKeys(vmPtr, gcId);\n");
    bootstrap.push_str("                const lowerProp = prop.toLowerCase();\n");
    bootstrap.push_str("                const matchedKey = keys.find(k => k.toLowerCase() === lowerProp);\n");
    bootstrap.push_str("                window.MatchBox.setInstanceProperty(vmPtr, gcId, matchedKey || prop, value);\n");
    bootstrap.push_str("                target[prop] = value;\n");
    bootstrap.push_str("                return true;\n");
    bootstrap.push_str("            },\n");
    bootstrap.push_str("            has(target, prop) {\n");
    bootstrap.push_str("                if (typeof prop !== \"string\") return prop in target;\n");
    bootstrap.push_str("                if (prop.startsWith(\"__matchbox_\")) return true;\n");
    bootstrap.push_str("                if (prop in target) return true;\n");
    bootstrap.push_str("                return window.MatchBox.getInstanceProperty(vmPtr, gcId, prop) !== undefined;\n");
    bootstrap.push_str("            },\n");
    bootstrap.push_str("            ownKeys(target) {\n");
    bootstrap.push_str("                const bxKeys = window.MatchBox.getInstanceKeys(vmPtr, gcId);\n");
    bootstrap.push_str("                const targetKeys = Reflect.ownKeys(target);\n");
    bootstrap.push_str("                const allKeys = new Set([...targetKeys, ...bxKeys]);\n");
    bootstrap.push_str("                return Array.from(allKeys);\n");
    bootstrap.push_str("            },\n");
    bootstrap.push_str("            getOwnPropertyDescriptor(target, prop) {\n");
    bootstrap.push_str("                if (typeof prop !== \"string\") return Reflect.getOwnPropertyDescriptor(target, prop);\n");
    bootstrap.push_str("                if (prop.startsWith(\"__matchbox_\")) return Reflect.getOwnPropertyDescriptor(target, prop);\n");
    bootstrap.push_str("                const value = window.MatchBox.getInstanceProperty(vmPtr, gcId, prop);\n");
    bootstrap.push_str("                if (value !== undefined) {\n");
    bootstrap.push_str("                    return {\n");
    bootstrap.push_str("                        enumerable: true,\n");
    bootstrap.push_str("                        configurable: true,\n");
    bootstrap.push_str("                        writable: true,\n");
    bootstrap.push_str("                        value\n");
    bootstrap.push_str("                    };\n");
    bootstrap.push_str("                }\n");
    bootstrap.push_str("                return Reflect.getOwnPropertyDescriptor(target, prop);\n");
    bootstrap.push_str("            }\n");
    bootstrap.push_str("        });\n");
    bootstrap.push_str("    };\n");
    bootstrap.push_str("    if (typeof BoxLangVM !== \"undefined\" && BoxLangVM.prototype && typeof BoxLangVM.prototype.free === \"function\") {\n");
    bootstrap.push_str("        const __matchboxOriginalFree = BoxLangVM.prototype.free;\n");
    bootstrap.push_str("        BoxLangVM.prototype.free = function() {\n");
    bootstrap.push_str("            const ptr = this.__matchbox_vm_ptr || this.__wbg_ptr;\n");
    bootstrap.push_str("            try {\n");
    bootstrap.push_str("                return __matchboxOriginalFree.call(this);\n");
    bootstrap.push_str("            } finally {\n");
    bootstrap.push_str("                if (ptr && window.MatchBox.unregisterCallbackBridge) {\n");
    bootstrap.push_str("                    window.MatchBox.unregisterCallbackBridge(ptr);\n");
    bootstrap.push_str("                }\n");
    bootstrap.push_str("            }\n");
    bootstrap.push_str("        };\n");
    bootstrap.push_str("        if (typeof Symbol !== \"undefined\" && Symbol.dispose) {\n");
    bootstrap.push_str("            BoxLangVM.prototype[Symbol.dispose] = BoxLangVM.prototype.free;\n");
    bootstrap.push_str("        }\n");
    bootstrap.push_str("    }\n");
    bootstrap.push_str("}\n\n");
    bootstrap.push_str("\nlet vm = null;\n");
    bootstrap.push_str("let __matchboxReady = null;\n");
    bootstrap.push_str("function isPlainObject(value) {\n");
    bootstrap.push_str("    return value != null && typeof value === \"object\" && !Array.isArray(value);\n");
    bootstrap.push_str("}\n\n");
    bootstrap.push_str("async function waitForModule(moduleName) {\n");
    bootstrap.push_str("    if (typeof window === \"undefined\") return null;\n");
    bootstrap.push_str("    const start = Date.now();\n");
    bootstrap.push_str("    while (Date.now() - start < 5000) {\n");
    bootstrap.push_str("        const mod = window.MatchBox?.modules?.[moduleName];\n");
    bootstrap.push_str("        if (mod) {\n");
    bootstrap.push_str("            return mod;\n");
    bootstrap.push_str("        }\n");
    bootstrap.push_str("        await new Promise(resolve => setTimeout(resolve, 25));\n");
    bootstrap.push_str("    }\n");
    bootstrap.push_str("    throw new Error(`MatchBox module ${moduleName} did not become ready`);\n");
    bootstrap.push_str("}\n\n");
    bootstrap.push_str("function createModuleState(moduleName, options = {}) {\n");
    bootstrap.push_str("    const initialState = options.initialState || {};\n");
    bootstrap.push_str("    const mount = options.mount || null;\n");
    bootstrap.push_str("    const state = {\n");
    bootstrap.push_str("        ready: false,\n");
    bootstrap.push_str("        error: null,\n");
    bootstrap.push_str("        ...initialState,\n");
    bootstrap.push_str("        async module() {\n");
    bootstrap.push_str("            if (typeof window !== \"undefined\" && typeof window.MatchBox?.ready === \"function\") {\n");
    bootstrap.push_str("                await window.MatchBox.ready(moduleName);\n");
    bootstrap.push_str("            }\n");
    bootstrap.push_str("            return await waitForModule(moduleName);\n");
    bootstrap.push_str("        },\n");
    bootstrap.push_str("        applyState(next) {\n");
    bootstrap.push_str("            if (!isPlainObject(next)) {\n");
    bootstrap.push_str("                return next;\n");
    bootstrap.push_str("            }\n");
    bootstrap.push_str("            for (const [key, value] of Object.entries(next)) {\n");
    bootstrap.push_str("                this[key] = value;\n");
    bootstrap.push_str("            }\n");
    bootstrap.push_str("            return next;\n");
    bootstrap.push_str("        },\n");
    bootstrap.push_str("        async call(method, ...args) {\n");
    bootstrap.push_str("            const mod = await this.module();\n");
    bootstrap.push_str("            if (typeof mod[method] !== \"function\") {\n");
    bootstrap.push_str("                throw new Error(`MatchBox export ${method} is not available on ${moduleName}`);\n");
    bootstrap.push_str("            }\n");
    bootstrap.push_str("            const result = await mod[method](...args);\n");
    bootstrap.push_str("            return this.applyState(result);\n");
    bootstrap.push_str("        },\n");
    bootstrap.push_str("        async init() {\n");
    bootstrap.push_str("            try {\n");
    bootstrap.push_str("                if (mount) {\n");
    bootstrap.push_str("                    await this.call(mount);\n");
    bootstrap.push_str("                }\n");
    bootstrap.push_str("                this.ready = true;\n");
    bootstrap.push_str("                this.error = null;\n");
    bootstrap.push_str("            } catch (error) {\n");
    bootstrap.push_str("                this.error = String(error);\n");
    bootstrap.push_str("            }\n");
    bootstrap.push_str("        }\n");
    bootstrap.push_str("    };\n");
    bootstrap.push_str("    return state;\n");
    bootstrap.push_str("}\n\n");
    bootstrap.push_str("async function ensureInit() {\n");
    bootstrap.push_str("    if (!__matchboxReady) {\n");
    bootstrap.push_str("        __matchboxReady = (async () => {\n");
    bootstrap.push_str("            await __wbg_init();\n");
    bootstrap.push_str("            if (!vm) {\n");
    bootstrap.push_str("                vm = new BoxLangVM();\n");
    bootstrap.push_str("                if (typeof window !== \"undefined\" && window.MatchBox && window.MatchBox.registerCallbackBridge) {\n");
    bootstrap.push_str("                    const vmPtr = vm.vm_ptr();\n");
    bootstrap.push_str("                    vm.__matchbox_vm_ptr = vmPtr;\n");
    bootstrap.push_str("                    window.MatchBox.registerCallbackBridge(vmPtr, _matchbox_invoke_callback);\n");
    bootstrap.push_str("                    if (window.MatchBox.registerPumpBridge && typeof vm.pump === \"function\") {\n");
    bootstrap.push_str("                        window.MatchBox.registerPumpBridge(vmPtr, () => vm.pump());\n");
    bootstrap.push_str("                    }\n");
    bootstrap.push_str("                }\n");
    bootstrap.push_str("                vm.init();\n");
    bootstrap.push_str("            }\n");
    bootstrap.push_str("            return vm;\n");
    bootstrap.push_str("        })();\n");
    bootstrap.push_str("    }\n");
    bootstrap.push_str("    return await __matchboxReady;\n");
    bootstrap.push_str("}\n\n");

    for func in functions {
        bootstrap.push_str(&format!("export async function {}(...args) {{\n", func));
        bootstrap.push_str("    const vm = await ensureInit();\n");
        bootstrap.push_str(&format!("    return await vm.call(\"{}\", args);\n", func));
        bootstrap.push_str("}\n\n");
    }

    bootstrap.push_str("if (typeof window !== \"undefined\") {\n");
    bootstrap.push_str("    window.MatchBox = window.MatchBox || {};\n");
    bootstrap.push_str("    window.MatchBox.runtime = window.MatchBox.runtime || \"browser\";\n");
    bootstrap.push_str("    window.MatchBox.contractVersion = window.MatchBox.contractVersion || 1;\n");
    bootstrap.push_str("    window.MatchBox.modules = window.MatchBox.modules || {};\n");
    bootstrap.push_str("    window.MatchBox._readySignals = window.MatchBox._readySignals || {};\n");
    bootstrap.push_str("    window.MatchBox.ready = window.MatchBox.ready || function(stem) {\n");
    bootstrap.push_str("        return window.MatchBox._readySignals[stem] || Promise.resolve();\n");
    bootstrap.push_str("    };\n");
    bootstrap.push_str("    window.MatchBox.createModuleState = window.MatchBox.createModuleState || createModuleState;\n");
    bootstrap.push_str("    window.MatchBox.State = window.MatchBox.State || createModuleState;\n");
    bootstrap.push_str(&format!("    window.MatchBox.modules[\"{}\"] = {{\n", module_name));
    for func in functions {
        bootstrap.push_str(&format!("        {},\n", func));
    }
    bootstrap.push_str("    };\n");
    bootstrap.push_str("}\n\n");

    bootstrap.push_str("export const ready = ensureInit();\n\n");

    bootstrap.push_str("if (typeof window !== \"undefined\") {\n");
    bootstrap.push_str(&format!("    window.MatchBox._readySignals[\"{}\"] = ready;\n", module_name));
    bootstrap.push_str("    ready.then(() => {\n");
    bootstrap.push_str("        if (typeof window.dispatchEvent === \"function\") {\n");
    bootstrap.push_str("            window.dispatchEvent(new CustomEvent(\"matchbox:ready\", {\n");
    bootstrap.push_str(&format!("                detail: {{ module: \"{}\" }}\n", module_name));
    bootstrap.push_str("            }));\n");
    bootstrap.push_str("        }\n");
    bootstrap.push_str("    });\n");
    bootstrap.push_str("}\n");

    bootstrap
}

/// Generates bootstrap JS for the stub-based (no-Cargo) build path.
/// Same callback bridge and MatchBox infrastructure as the fusion bootstrap,
/// but uses `load_bytecode()` with base64-encoded bytecode instead of `init()`.
pub fn render_stub_js_bootstrap(functions: &[String], module_name: &str, b64_bytecode: &str) -> String {
    let mut bootstrap = String::new();

    // Base64 bytecode constant
    bootstrap.push_str(&format!("const __matchboxBytecodeBase64 = \"{}\";\n\n", b64_bytecode));

    // Callback, pump, and instance bridge setup
    bootstrap.push_str("if (typeof window !== \"undefined\") {\n");
    bootstrap.push_str("    window.MatchBox = window.MatchBox || {};\n");
    bootstrap.push_str("    window.MatchBox._callbackBridges = window.MatchBox._callbackBridges || new Map();\n");
    bootstrap.push_str("    window.MatchBox._pumpBridges = window.MatchBox._pumpBridges || new Map();\n");
    bootstrap.push_str("    window.MatchBox.registerCallbackBridge = window.MatchBox.registerCallbackBridge || function(vmPtr, bridge) {\n");
    bootstrap.push_str("        window.MatchBox._callbackBridges.set(vmPtr, bridge);\n");
    bootstrap.push_str("    };\n");
    bootstrap.push_str("    window.MatchBox.registerPumpBridge = window.MatchBox.registerPumpBridge || function(vmPtr, bridge) {\n");
    bootstrap.push_str("        window.MatchBox._pumpBridges.set(vmPtr, bridge);\n");
    bootstrap.push_str("    };\n");
    bootstrap.push_str("    window.MatchBox.unregisterCallbackBridge = window.MatchBox.unregisterCallbackBridge || function(vmPtr) {\n");
    bootstrap.push_str("        window.MatchBox._callbackBridges.delete(vmPtr);\n");
    bootstrap.push_str("        window.MatchBox._pumpBridges.delete(vmPtr);\n");
    bootstrap.push_str("    };\n");
    bootstrap.push_str("    window.MatchBox.invokeCallback = window.MatchBox.invokeCallback || function(vmPtr, callbackId, thisVal, args) {\n");
    bootstrap.push_str("        const bridge = window.MatchBox._callbackBridges.get(vmPtr);\n");
    bootstrap.push_str("        if (!bridge) {\n");
    bootstrap.push_str("            throw new Error(\"MatchBox callback bridge is not registered for VM \" + vmPtr);\n");
    bootstrap.push_str("        }\n");
    bootstrap.push_str("        return bridge(vmPtr, callbackId, thisVal, args);\n");
    bootstrap.push_str("    };\n");
    bootstrap.push_str("    window.MatchBox.schedulePump = window.MatchBox.schedulePump || function(vmPtr) {\n");
    bootstrap.push_str("        const bridge = window.MatchBox._pumpBridges.get(vmPtr);\n");
    bootstrap.push_str("        if (!bridge) {\n");
    bootstrap.push_str("            return;\n");
    bootstrap.push_str("        }\n");
    bootstrap.push_str("        const pump = () => {\n");
    bootstrap.push_str("            const bridge = window.MatchBox._pumpBridges.get(vmPtr);\n");
    bootstrap.push_str("            if (bridge) {\n");
    bootstrap.push_str("                try {\n");
    bootstrap.push_str("                    bridge();\n");
    bootstrap.push_str("                } catch (error) {\n");
    bootstrap.push_str("                    if (typeof console !== \"undefined\" && console.error) {\n");
    bootstrap.push_str("                        console.error(\"MatchBox scheduled pump failed\", error);\n");
    bootstrap.push_str("                    }\n");
    bootstrap.push_str("                }\n");
    bootstrap.push_str("            }\n");
    bootstrap.push_str("        };\n");
    bootstrap.push_str("        if (typeof queueMicrotask === \"function\") {\n");
    bootstrap.push_str("            queueMicrotask(pump);\n");
    bootstrap.push_str("        } else {\n");
    bootstrap.push_str("            setTimeout(pump, 0);\n");
    bootstrap.push_str("        }\n");
    bootstrap.push_str("    };\n");
    bootstrap.push_str("    window.MatchBox.getInstanceProperty = window.MatchBox.getInstanceProperty || function(vmPtr, gcId, name) {\n");
    bootstrap.push_str("        if (typeof _matchbox_get_instance_prop !== \"function\") return undefined;\n");
    bootstrap.push_str("        return _matchbox_get_instance_prop(vmPtr, gcId, name);\n");
    bootstrap.push_str("    };\n");
    bootstrap.push_str("    window.MatchBox.getInstanceKeys = window.MatchBox.getInstanceKeys || function(vmPtr, gcId) {\n");
    bootstrap.push_str("        if (typeof _matchbox_get_instance_keys !== \"function\") return [];\n");
    bootstrap.push_str("        return _matchbox_get_instance_keys(vmPtr, gcId);\n");
    bootstrap.push_str("    };\n");
    bootstrap.push_str("    window.MatchBox.setInstanceProperty = window.MatchBox.setInstanceProperty || function(vmPtr, gcId, name, value) {\n");
    bootstrap.push_str("        if (typeof _matchbox_set_instance_prop !== \"function\") return;\n");
    bootstrap.push_str("        _matchbox_set_instance_prop(vmPtr, gcId, name, value);\n");
    bootstrap.push_str("    };\n");
    bootstrap.push_str(r#"    window.MatchBox.wrapInstancePropertyValue = window.MatchBox.wrapInstancePropertyValue || function(vmPtr, gcId, prop, value, ownerReceiver) {
        if (value == null || typeof value !== "object") return value;
        if (ArrayBuffer.isView(value) || value instanceof ArrayBuffer || value instanceof Date || value instanceof RegExp || value instanceof Promise) {
            return value;
        }
        if (value.__matchbox_nested_proxy__) return value;
        const persist = () => {
            if (ownerReceiver != null && typeof ownerReceiver === "object") {
                Reflect.set(ownerReceiver, prop, value, ownerReceiver);
            } else {
                window.MatchBox.setInstanceProperty(vmPtr, gcId, prop, value);
            }
        };
        const wrapChild = child => window.MatchBox.wrapInstancePropertyValue(vmPtr, gcId, prop, child, ownerReceiver);
        for (const key of Reflect.ownKeys(value)) {
            const current = value[key];
            if (current != null && typeof current === "object") {
                value[key] = wrapChild(current);
            }
        }
        return new Proxy(value, {
            get(target, nestedProp, receiver) {
                if (nestedProp === "__matchbox_nested_proxy__") return true;
                const nestedValue = Reflect.get(target, nestedProp, target);
                return nestedValue != null && typeof nestedValue === "object" ? wrapChild(nestedValue) : nestedValue;
            },
            set(target, nestedProp, nestedValue, receiver) {
                const wrapped = nestedValue != null && typeof nestedValue === "object" ? wrapChild(nestedValue) : nestedValue;
                const result = Reflect.set(target, nestedProp, wrapped, receiver);
                persist();
                return result;
            },
            deleteProperty(target, nestedProp) {
                const result = Reflect.deleteProperty(target, nestedProp);
                persist();
                return result;
            }
        });
    };
"#);
    bootstrap.push_str("    window.MatchBox.createInstanceProxy = window.MatchBox.createInstanceProxy || function(vmPtr, gcId) {\n");
    bootstrap.push_str("        const target = { __matchbox_vm_ptr: vmPtr, __matchbox_gc_id: gcId, __matchbox_cache: {} };\n");
    bootstrap.push_str("        return new Proxy(target, {\n");
    bootstrap.push_str("            get(target, prop, receiver) {\n");
    bootstrap.push_str("                if (typeof prop !== \"string\") return target[prop];\n");
    bootstrap.push_str("                if (prop.startsWith(\"__matchbox_\")) return target[prop];\n");
    bootstrap.push_str("                if (target.__matchbox_cache[prop]) return target.__matchbox_cache[prop];\n");
    bootstrap.push_str("                let val = window.MatchBox.getInstanceProperty(vmPtr, gcId, prop);\n");
    bootstrap.push_str("                if (val === undefined) {\n");
    bootstrap.push_str("                    const keys = window.MatchBox.getInstanceKeys(vmPtr, gcId);\n");
    bootstrap.push_str("                    const lowerProp = prop.toLowerCase();\n");
    bootstrap.push_str("                    const matchedKey = keys.find(k => k.toLowerCase() === lowerProp);\n");
    bootstrap.push_str("                    if (matchedKey && matchedKey !== prop) {\n");
    bootstrap.push_str("                        val = window.MatchBox.getInstanceProperty(vmPtr, gcId, matchedKey);\n");
    bootstrap.push_str("                    }\n");
    bootstrap.push_str("                }\n");
    bootstrap.push_str("                if (typeof val === \"function\") {\n");
    bootstrap.push_str("                    // Keep methods unbound so `this` stays on the actual JS receiver.\n");
    bootstrap.push_str("                    // That lets reactive wrappers observe BoxLang instance writes.\n");
    bootstrap.push_str("                    target.__matchbox_cache[prop] = val;\n");
    bootstrap.push_str("                } else if (val != null && typeof val === \"object\") {\n");
    bootstrap.push_str("                    val = window.MatchBox.wrapInstancePropertyValue(vmPtr, gcId, prop, val, receiver);\n");
    bootstrap.push_str("                }\n");
    bootstrap.push_str("                return val !== undefined ? val : target[prop];\n");
    bootstrap.push_str("            },\n");
    bootstrap.push_str("            set(target, prop, value) {\n");
    bootstrap.push_str("                if (typeof prop !== \"string\") return false;\n");
    bootstrap.push_str("                if (prop.startsWith(\"__matchbox_\")) { target[prop] = value; return true; }\n");
    bootstrap.push_str("                const keys = window.MatchBox.getInstanceKeys(vmPtr, gcId);\n");
    bootstrap.push_str("                const lowerProp = prop.toLowerCase();\n");
    bootstrap.push_str("                const matchedKey = keys.find(k => k.toLowerCase() === lowerProp);\n");
    bootstrap.push_str("                window.MatchBox.setInstanceProperty(vmPtr, gcId, matchedKey || prop, value);\n");
    bootstrap.push_str("                target[prop] = value;\n");
    bootstrap.push_str("                return true;\n");
    bootstrap.push_str("            },\n");
    bootstrap.push_str("            has(target, prop) {\n");
    bootstrap.push_str("                if (typeof prop !== \"string\") return prop in target;\n");
    bootstrap.push_str("                if (prop.startsWith(\"__matchbox_\")) return true;\n");
    bootstrap.push_str("                if (prop in target) return true;\n");
    bootstrap.push_str("                const keys = window.MatchBox.getInstanceKeys(vmPtr, gcId);\n");
    bootstrap.push_str("                const lowerProp = prop.toLowerCase();\n");
    bootstrap.push_str("                return keys.some(k => k.toLowerCase() === lowerProp);\n");
    bootstrap.push_str("            },\n");
    bootstrap.push_str("            ownKeys(target) {\n");
    bootstrap.push_str("                const bxKeys = window.MatchBox.getInstanceKeys(vmPtr, gcId);\n");
    bootstrap.push_str("                const targetKeys = Reflect.ownKeys(target);\n");
    bootstrap.push_str("                const allKeys = new Set([...targetKeys, ...bxKeys]);\n");
    bootstrap.push_str("                return Array.from(allKeys);\n");
    bootstrap.push_str("            },\n");
    bootstrap.push_str("            getOwnPropertyDescriptor(target, prop) {\n");
    bootstrap.push_str("                if (typeof prop !== \"string\") return Reflect.getOwnPropertyDescriptor(target, prop);\n");
    bootstrap.push_str("                if (prop.startsWith(\"__matchbox_\")) return Reflect.getOwnPropertyDescriptor(target, prop);\n");
    bootstrap.push_str("                const keys = window.MatchBox.getInstanceKeys(vmPtr, gcId);\n");
    bootstrap.push_str("                if (keys.includes(prop)) {\n");
    bootstrap.push_str("                    return {\n");
    bootstrap.push_str("                        enumerable: true,\n");
    bootstrap.push_str("                        configurable: true,\n");
    bootstrap.push_str("                        writable: true,\n");
    bootstrap.push_str("                        value: this.get(target, prop)\n");
    bootstrap.push_str("                    };\n");
    bootstrap.push_str("                }\n");
    bootstrap.push_str("                return Reflect.getOwnPropertyDescriptor(target, prop);\n");
    bootstrap.push_str("            }\n");
    bootstrap.push_str("        });\n");
    bootstrap.push_str("    };\n");
    bootstrap.push_str("    if (typeof BoxLangVM !== \"undefined\" && BoxLangVM.prototype && typeof BoxLangVM.prototype.free === \"function\") {\n");
    bootstrap.push_str("        const __matchboxOriginalFree = BoxLangVM.prototype.free;\n");
    bootstrap.push_str("        BoxLangVM.prototype.free = function() {\n");
    bootstrap.push_str("            const ptr = this.__matchbox_vm_ptr || this.__wbg_ptr;\n");
    bootstrap.push_str("            try {\n");
    bootstrap.push_str("                return __matchboxOriginalFree.call(this);\n");
    bootstrap.push_str("            } finally {\n");
    bootstrap.push_str("                if (ptr && window.MatchBox.unregisterCallbackBridge) {\n");
    bootstrap.push_str("                    window.MatchBox.unregisterCallbackBridge(ptr);\n");
    bootstrap.push_str("                }\n");
    bootstrap.push_str("            }\n");
    bootstrap.push_str("        };\n");
    bootstrap.push_str("        if (typeof Symbol !== \"undefined\" && Symbol.dispose) {\n");
    bootstrap.push_str("            BoxLangVM.prototype[Symbol.dispose] = BoxLangVM.prototype.free;\n");
    bootstrap.push_str("        }\n");
    bootstrap.push_str("    }\n");
    bootstrap.push_str("}\n\n");

    bootstrap.push_str("let vm = null;\n");
    bootstrap.push_str("let __matchboxReady = null;\n");
    bootstrap.push_str("function isPlainObject(value) {\n");
    bootstrap.push_str("    return value != null && typeof value === \"object\" && !Array.isArray(value);\n");
    bootstrap.push_str("}\n\n");

    bootstrap.push_str("async function waitForModule(moduleName) {\n");
    bootstrap.push_str("    if (typeof window === \"undefined\") return null;\n");
    bootstrap.push_str("    const start = Date.now();\n");
    bootstrap.push_str("    while (Date.now() - start < 5000) {\n");
    bootstrap.push_str("        const mod = window.MatchBox?.modules?.[moduleName];\n");
    bootstrap.push_str("        if (mod) {\n");
    bootstrap.push_str("            return mod;\n");
    bootstrap.push_str("        }\n");
    bootstrap.push_str("        await new Promise(resolve => setTimeout(resolve, 25));\n");
    bootstrap.push_str("    }\n");
    bootstrap.push_str("    throw new Error(`MatchBox module ${moduleName} did not become ready`);\n");
    bootstrap.push_str("}\n\n");

    bootstrap.push_str("function createModuleState(moduleName, options = {}) {\n");
    bootstrap.push_str("    const initialState = options.initialState || {};\n");
    bootstrap.push_str("    const mount = options.mount || null;\n");
    bootstrap.push_str("    const state = {\n");
    bootstrap.push_str("        ready: false,\n");
    bootstrap.push_str("        error: null,\n");
    bootstrap.push_str("        ...initialState,\n");
    bootstrap.push_str("        async module() {\n");
    bootstrap.push_str("            if (typeof window !== \"undefined\" && typeof window.MatchBox?.ready === \"function\") {\n");
    bootstrap.push_str("                await window.MatchBox.ready(moduleName);\n");
    bootstrap.push_str("            }\n");
    bootstrap.push_str("            return await waitForModule(moduleName);\n");
    bootstrap.push_str("        },\n");
    bootstrap.push_str("        applyState(next) {\n");
    bootstrap.push_str("            if (!isPlainObject(next)) {\n");
    bootstrap.push_str("                return next;\n");
    bootstrap.push_str("            }\n");
    bootstrap.push_str("            for (const [key, value] of Object.entries(next)) {\n");
    bootstrap.push_str("                this[key] = value;\n");
    bootstrap.push_str("            }\n");
    bootstrap.push_str("            return next;\n");
    bootstrap.push_str("        },\n");
    bootstrap.push_str("        async call(method, ...args) {\n");
    bootstrap.push_str("            const mod = await this.module();\n");
    bootstrap.push_str("            if (typeof mod[method] !== \"function\") {\n");
    bootstrap.push_str("                throw new Error(`MatchBox export ${method} is not available on ${moduleName}`);\n");
    bootstrap.push_str("            }\n");
    bootstrap.push_str("            const result = await mod[method](...args);\n");
    bootstrap.push_str("            return this.applyState(result);\n");
    bootstrap.push_str("        },\n");
    bootstrap.push_str("        async init() {\n");
    bootstrap.push_str("            try {\n");
    bootstrap.push_str("                if (mount) {\n");
    bootstrap.push_str("                    await this.call(mount);\n");
    bootstrap.push_str("                }\n");
    bootstrap.push_str("                this.ready = true;\n");
    bootstrap.push_str("                this.error = null;\n");
    bootstrap.push_str("            } catch (error) {\n");
    bootstrap.push_str("                this.error = String(error);\n");
    bootstrap.push_str("            }\n");
    bootstrap.push_str("        }\n");
    bootstrap.push_str("    };\n");
    bootstrap.push_str("    return state;\n");
    bootstrap.push_str("}\n\n");

    // ensureInit — uses load_bytecode() instead of init()
    bootstrap.push_str("async function ensureInit() {\n");
    bootstrap.push_str("    if (!__matchboxReady) {\n");
    bootstrap.push_str("        __matchboxReady = (async () => {\n");
    bootstrap.push_str("            await __wbg_init();\n");
    bootstrap.push_str("            if (!vm) {\n");
    bootstrap.push_str("                vm = new BoxLangVM();\n");
    bootstrap.push_str("                if (typeof window !== \"undefined\" && window.MatchBox && window.MatchBox.registerCallbackBridge) {\n");
    bootstrap.push_str("                    const vmPtr = vm.vm_ptr();\n");
    bootstrap.push_str("                    vm.__matchbox_vm_ptr = vmPtr;\n");
    bootstrap.push_str("                    window.MatchBox.registerCallbackBridge(vmPtr, _matchbox_invoke_callback);\n");
    bootstrap.push_str("                    if (window.MatchBox.registerPumpBridge && typeof vm.pump === \"function\") {\n");
    bootstrap.push_str("                        window.MatchBox.registerPumpBridge(vmPtr, () => vm.pump());\n");
    bootstrap.push_str("                    }\n");
    bootstrap.push_str("                }\n");
    bootstrap.push_str("                const bytecodeBytes = Uint8Array.from(atob(__matchboxBytecodeBase64), c => c.charCodeAt(0));\n");
    bootstrap.push_str("                vm.load_bytecode(bytecodeBytes);\n");
    bootstrap.push_str("            }\n");
    bootstrap.push_str("            return vm;\n");
    bootstrap.push_str("        })();\n");
    bootstrap.push_str("    }\n");
    bootstrap.push_str("    return await __matchboxReady;\n");
    bootstrap.push_str("}\n\n");

    for func in functions {
        bootstrap.push_str(&format!("export async function {}(...args) {{\n", func));
        bootstrap.push_str("    const vm = await ensureInit();\n");
        bootstrap.push_str(&format!("    return await vm.call(\"{}\", args);\n", func));
        bootstrap.push_str("}\n\n");
    }

    bootstrap.push_str("if (typeof window !== \"undefined\") {\n");
    bootstrap.push_str("    window.MatchBox = window.MatchBox || {};\n");
    bootstrap.push_str("    window.MatchBox.runtime = window.MatchBox.runtime || \"browser\";\n");
    bootstrap.push_str("    window.MatchBox.contractVersion = window.MatchBox.contractVersion || 1;\n");
    bootstrap.push_str("    window.MatchBox.modules = window.MatchBox.modules || {};\n");
    bootstrap.push_str("    window.MatchBox._readySignals = window.MatchBox._readySignals || {};\n");
    bootstrap.push_str("    window.MatchBox.ready = window.MatchBox.ready || function(stem) {\n");
    bootstrap.push_str("        return window.MatchBox._readySignals[stem] || Promise.resolve();\n");
    bootstrap.push_str("    };\n");
    bootstrap.push_str("    window.MatchBox.createModuleState = window.MatchBox.createModuleState || createModuleState;\n");
    bootstrap.push_str("    window.MatchBox.State = window.MatchBox.State || createModuleState;\n");
    bootstrap.push_str(&format!("    window.MatchBox.modules[\"{}\"] = {{\n", module_name));
    for func in functions {
        bootstrap.push_str(&format!("        {},\n", func));
    }
    bootstrap.push_str("    };\n");
    bootstrap.push_str("}\n\n");

    bootstrap.push_str("export const ready = ensureInit();\n\n");

    bootstrap.push_str("if (typeof window !== \"undefined\") {\n");
    bootstrap.push_str(&format!("    window.MatchBox._readySignals[\"{}\"] = ready;\n", module_name));
    bootstrap.push_str("    ready.then(() => {\n");
    bootstrap.push_str("        if (typeof window.dispatchEvent === \"function\") {\n");
    bootstrap.push_str("            window.dispatchEvent(new CustomEvent(\"matchbox:ready\", {\n");
    bootstrap.push_str(&format!("                detail: {{ module: \"{}\" }}\n", module_name));
    bootstrap.push_str("            }));\n");
    bootstrap.push_str("        }\n");
    bootstrap.push_str("    });\n");
    bootstrap.push_str("}\n");

    bootstrap
}

#[cfg(test)]
mod tests {
    use super::*;
    use matchbox_compiler::parser;

    #[test]
    fn exported_function_names_collects_top_level_functions() {
        let source = r#"
            function alpha() {}
            x = 1
            function beta(a) { return a; }
        "#;
        let ast = parser::parse(source, Some("bootstrap_test")).unwrap();
        assert_eq!(exported_function_names(&ast), vec!["alpha".to_string(), "beta".to_string()]);
    }

    #[test]
    fn pure_js_bootstrap_keeps_stub_loader_shape() {
        let bootstrap = render_pure_js_bootstrap(
            &vec!["hello".to_string()],
            "stub-wasm",
            "stub-bytecode",
        );

        assert!(bootstrap.contains("const wasmBase64 = \"stub-wasm\";"));
        assert!(bootstrap.contains("const bytecodeBase64 = \"stub-bytecode\";"));
        assert!(bootstrap.contains("vm.load_bytecode(bytecodeBinary);"));
        assert!(bootstrap.contains("return await vm.call(\"hello\", args);"));
        assert!(bootstrap.contains("if (e instanceof Error) throw e;"));
    }

    #[test]
    fn fusion_js_bootstrap_uses_async_vm_call_without_bytecode_loader() {
        let bootstrap = render_fusion_js_bootstrap(&vec!["hello".to_string()], "app");

        assert!(bootstrap.contains("registerCallbackBridge"));
        assert!(bootstrap.contains("invokeCallback"));
        assert!(bootstrap.contains("BoxLangVM.prototype.free"));
        assert!(bootstrap.contains("await __wbg_init();"));
        assert!(bootstrap.contains("vm = new BoxLangVM();"));
        assert!(bootstrap.contains("vm.init();"));
        assert!(bootstrap.contains("return await vm.call(\"hello\", args);"));
        assert!(bootstrap.contains("window.MatchBox.modules[\"app\"]"));
        assert!(bootstrap.contains("hello,"));
        assert!(!bootstrap.contains("load_bytecode"));
        assert!(!bootstrap.contains("wasmBase64"));
    }

    #[test]
    fn stub_js_bootstrap_uses_load_bytecode_with_callback_bridge() {
        let bootstrap = render_stub_js_bootstrap(&vec!["hello".to_string()], "app", "AQID");

        assert!(bootstrap.contains("__matchboxBytecodeBase64 = \"AQID\""));
        assert!(bootstrap.contains("registerCallbackBridge"));
        assert!(bootstrap.contains("registerPumpBridge"));
        assert!(bootstrap.contains("invokeCallback"));
        assert!(bootstrap.contains("schedulePump"));
        assert!(bootstrap.contains("createInstanceProxy"));
        assert!(bootstrap.contains("getInstanceProperty"));
        assert!(bootstrap.contains("getInstanceKeys"));
        assert!(bootstrap.contains("setInstanceProperty"));
        assert!(bootstrap.contains("wrapInstancePropertyValue"));
        assert!(bootstrap.contains("vm = new BoxLangVM();"));
        assert!(bootstrap.contains("vm.load_bytecode(bytecodeBytes);"));
        assert!(bootstrap.contains("return await vm.call(\"hello\", args);"));
        assert!(bootstrap.contains("window.MatchBox.modules[\"app\"]"));
        assert!(bootstrap.contains("hello,"));
        assert!(!bootstrap.contains("vm.init();"));
    }
}
