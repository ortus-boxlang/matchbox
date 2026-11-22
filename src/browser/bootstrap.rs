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

#[allow(dead_code)]
pub fn render_pure_js_bootstrap(
    functions: &[String],
    b64_wasm: &str,
    b64_bytecode: &str,
) -> String {
    let mut bootstrap = String::new();
    bootstrap.push_str(&format!("const wasmBase64 = \"{}\";\n", b64_wasm));
    bootstrap.push_str(&format!("const bytecodeBase64 = \"{}\";\n\n", b64_bytecode));

    bootstrap.push_str("let vm = null;\n");
    bootstrap.push_str("async function ensureInit() {\n");
    bootstrap.push_str("    if (vm) return;\n");
    bootstrap.push_str(
        "    const wasmBinary = Uint8Array.from(atob(wasmBase64), c => c.charCodeAt(0));\n",
    );
    bootstrap.push_str("    await init(wasmBinary);\n");
    bootstrap.push_str("    vm = new BoxLangVM();\n");
    bootstrap.push_str(
        "    const bytecodeBinary = Uint8Array.from(atob(bytecodeBase64), c => c.charCodeAt(0));\n",
    );
    bootstrap.push_str("    vm.load_bytecode(bytecodeBinary);\n");
    bootstrap.push_str("}\n\n");

    for func in functions {
        bootstrap.push_str(&format!("export async function {}(...args) {{\n", func));
        bootstrap.push_str("    await ensureInit();\n");
        bootstrap.push_str(&format!("    try {{\n"));
        bootstrap.push_str(&format!(
            "        return await vm.call(\"{}\", args);\n",
            func
        ));
        bootstrap.push_str(&format!("    }} catch (e) {{\n"));
        bootstrap.push_str(&format!(
            "        if (typeof console !== \"undefined\" && console.error) {{\n"
        ));
        bootstrap.push_str(&format!(
            "            console.error(\"[matchbox] VM call to '{}' failed:\", e);\n",
            func
        ));
        bootstrap.push_str(&format!("        }}\n"));
        bootstrap.push_str(&format!("        if (e instanceof Error) throw e;\n"));
        bootstrap.push_str(&format!("        throw new Error(String(e));\n"));
        bootstrap.push_str(&format!("    }}\n"));
        bootstrap.push_str("}\n\n");
    }

    bootstrap
}

pub fn render_fusion_js_bootstrap(functions: &[String], module_name: &str) -> String {
    let mut bootstrap = String::new();
    bootstrap.push_str(r#"if (typeof window !== "undefined") {
    window.MatchBox = window.MatchBox || {};
    window.MatchBox._hosts = window.MatchBox._hosts || new Map();
    window.MatchBox._nextHostId = window.MatchBox._nextHostId || 1;
    window.MatchBox.registerHost = window.MatchBox.registerHost || function(host) {
        const hostId = window.MatchBox._nextHostId++;
        window.MatchBox._hosts.set(hostId, host);
        return hostId;
    };
    window.MatchBox.unregisterHost = window.MatchBox.unregisterHost || function(hostId) {
        window.MatchBox._hosts.delete(hostId);
    };
    window.MatchBox._host = window.MatchBox._host || function(hostId) {
        const host = window.MatchBox._hosts.get(hostId);
        if (!host) {
            throw new Error("MatchBox host is not registered: " + hostId);
        }
        return host;
    };
    window.MatchBox.invokeCallback = window.MatchBox.invokeCallback || function(hostId, callbackId, thisVal, args) {
        return window.MatchBox._host(hostId).invoke(callbackId, thisVal, args);
    };
    window.MatchBox.schedulePump = window.MatchBox.schedulePump || function(hostId) {
        if (!window.MatchBox._hosts.has(hostId)) {
            return;
        }
        const pump = () => {
            const host = window.MatchBox._hosts.get(hostId);
            if (host) {
                try {
                    host.pump();
                } catch (error) {
                    if (typeof console !== "undefined" && console.error) {
                        console.error("MatchBox scheduled pump failed", error);
                    }
                }
            }
        };
        if (typeof queueMicrotask === "function") {
            queueMicrotask(pump);
        } else {
            setTimeout(pump, 0);
        }
    };
    window.MatchBox.getInstanceProperty = window.MatchBox.getInstanceProperty || function(hostId, gcId, name) {
        return window.MatchBox._host(hostId).getProp(gcId, name);
    };
    window.MatchBox.getInstanceKeys = window.MatchBox.getInstanceKeys || function(hostId, gcId) {
        return window.MatchBox._host(hostId).getKeys(gcId);
    };
    window.MatchBox.setInstanceProperty = window.MatchBox.setInstanceProperty || function(hostId, gcId, name, value) {
        window.MatchBox._host(hostId).setProp(gcId, name, value);
    };
"#);
    bootstrap.push_str("    window.MatchBox.__matchbox_proxy_targets = window.MatchBox.__matchbox_proxy_targets || new WeakMap();\n");
    bootstrap.push_str(r#"    window.MatchBox.wrapInstancePropertyValue = window.MatchBox.wrapInstancePropertyValue || function(hostId, gcId, prop, value, ownerReceiver) {
        if (value == null || typeof value !== "object") return value;
        if (ArrayBuffer.isView(value) || value instanceof ArrayBuffer || value instanceof Date || value instanceof RegExp || value instanceof Promise) {
            return value;
        }
        if (value.__matchbox_nested_proxy__) return value;
        const persist = () => {
            if (ownerReceiver != null && typeof ownerReceiver === "object") {
                Reflect.set(ownerReceiver, prop, value, ownerReceiver);
            } else {
                window.MatchBox.setInstanceProperty(hostId, gcId, prop, value);
            }
        };
        const wrapChild = child => window.MatchBox.wrapInstancePropertyValue(hostId, gcId, prop, child, ownerReceiver);
        for (const key of Reflect.ownKeys(value)) {
            const current = value[key];
            if (current != null && typeof current === "object") {
                value[key] = wrapChild(current);
            }
        }
        const proxy = new Proxy(value, {
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
        window.MatchBox.__matchbox_proxy_targets.set(proxy, value);
        return proxy;
    };
"#);
    bootstrap.push_str("    window.MatchBox.createInstanceProxy = window.MatchBox.createInstanceProxy || function(hostId, gcId) {\n");
    bootstrap.push_str("        const target = { __matchbox_host_id: hostId, __matchbox_gc_id: gcId, __matchbox_cache: {} };\n");
    bootstrap.push_str("        return new Proxy(target, {\n");
    bootstrap.push_str("            get(target, prop, receiver) {\n");
    bootstrap.push_str("                if (typeof prop !== \"string\") return target[prop];\n");
    bootstrap
        .push_str("                if (prop.startsWith(\"__matchbox_\")) return target[prop];\n");
    bootstrap.push_str("                if (target.__matchbox_cache[prop]) return target.__matchbox_cache[prop];\n");
    bootstrap.push_str(
        "                let val = window.MatchBox.getInstanceProperty(hostId, gcId, prop);\n",
    );
    bootstrap.push_str("                if (val === undefined) {\n");
    bootstrap.push_str(
        "                    const keys = window.MatchBox.getInstanceKeys(hostId, gcId);\n",
    );
    bootstrap.push_str("                    const lowerProp = prop.toLowerCase();\n");
    bootstrap.push_str(
        "                    const matchedKey = keys.find(k => k.toLowerCase() === lowerProp);\n",
    );
    bootstrap.push_str("                    if (matchedKey && matchedKey !== prop) {\n");
    bootstrap.push_str("                        val = window.MatchBox.getInstanceProperty(hostId, gcId, matchedKey);\n");
    bootstrap.push_str("                    }\n");
    bootstrap.push_str("                }\n");
    bootstrap.push_str("                if (typeof val === \"function\") {\n");
    bootstrap.push_str(
        "                    // Keep methods unbound so `this` stays on the actual JS receiver.\n",
    );
    bootstrap.push_str(
        "                    // That lets reactive wrappers observe BoxLang instance writes.\n",
    );
    bootstrap.push_str("                    target.__matchbox_cache[prop] = val;\n");
    bootstrap.push_str("                } else if (val != null && typeof val === \"object\") {\n");
    bootstrap.push_str("                    val = window.MatchBox.wrapInstancePropertyValue(hostId, gcId, prop, val, receiver);\n");
    bootstrap.push_str("                }\n");
    bootstrap.push_str("                return val !== undefined ? val : target[prop];\n");
    bootstrap.push_str("            },\n");
    bootstrap.push_str("            set(target, prop, value) {\n");
    bootstrap.push_str("                if (typeof prop !== \"string\") return false;\n");
    bootstrap.push_str("                if (prop.startsWith(\"__matchbox_\")) { target[prop] = value; return true; }\n");
    bootstrap
        .push_str("                const keys = window.MatchBox.getInstanceKeys(hostId, gcId);\n");
    bootstrap.push_str("                const lowerProp = prop.toLowerCase();\n");
    bootstrap.push_str(
        "                const matchedKey = keys.find(k => k.toLowerCase() === lowerProp);\n",
    );
    bootstrap.push_str("                window.MatchBox.setInstanceProperty(hostId, gcId, matchedKey || prop, value);\n");
    bootstrap.push_str("                target[prop] = value;\n");
    bootstrap.push_str("                return true;\n");
    bootstrap.push_str("            },\n");
    bootstrap.push_str("            has(target, prop) {\n");
    bootstrap.push_str("                if (typeof prop !== \"string\") return prop in target;\n");
    bootstrap.push_str("                if (prop.startsWith(\"__matchbox_\")) return true;\n");
    bootstrap.push_str("                if (prop in target) return true;\n");
    bootstrap.push_str("                return window.MatchBox.getInstanceProperty(hostId, gcId, prop) !== undefined;\n");
    bootstrap.push_str("            },\n");
    bootstrap.push_str("            ownKeys(target) {\n");
    bootstrap
        .push_str("                const bxKeys = window.MatchBox.getInstanceKeys(hostId, gcId);\n");
    bootstrap.push_str("                const targetKeys = Reflect.ownKeys(target);\n");
    bootstrap.push_str("                const allKeys = new Set([...targetKeys, ...bxKeys]);\n");
    bootstrap.push_str("                return Array.from(allKeys);\n");
    bootstrap.push_str("            },\n");
    bootstrap.push_str("            getOwnPropertyDescriptor(target, prop) {\n");
    bootstrap.push_str("                if (typeof prop !== \"string\") return Reflect.getOwnPropertyDescriptor(target, prop);\n");
    bootstrap.push_str("                if (prop.startsWith(\"__matchbox_\")) return Reflect.getOwnPropertyDescriptor(target, prop);\n");
    bootstrap.push_str(
        "                const value = window.MatchBox.getInstanceProperty(hostId, gcId, prop);\n",
    );
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
    bootstrap.push_str("            const ptr = this.__matchbox_host_id;\n");
    bootstrap.push_str("            try {\n");
    bootstrap.push_str("                return __matchboxOriginalFree.call(this);\n");
    bootstrap.push_str("            } finally {\n");
    bootstrap.push_str("                if (ptr && window.MatchBox.unregisterHost) {\n");
    bootstrap.push_str("                    window.MatchBox.unregisterHost(ptr);\n");
    bootstrap.push_str("                }\n");
    bootstrap.push_str("            }\n");
    bootstrap.push_str("        };\n");
    bootstrap.push_str("        if (typeof Symbol !== \"undefined\" && Symbol.dispose) {\n");
    bootstrap
        .push_str("            BoxLangVM.prototype[Symbol.dispose] = BoxLangVM.prototype.free;\n");
    bootstrap.push_str("        }\n");
    bootstrap.push_str("    }\n");
    bootstrap.push_str("}\n\n");
    bootstrap.push_str("\nlet vm = null;\n");
    bootstrap.push_str("let __matchboxReady = null;\n");
    bootstrap.push_str("function isPlainObject(value) {\n");
    bootstrap.push_str(
        "    return value != null && typeof value === \"object\" && !Array.isArray(value);\n",
    );
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
    bootstrap
        .push_str("    throw new Error(`MatchBox module ${moduleName} did not become ready`);\n");
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
    // Non-browser hosts (Cloudflare Workers, other WASM-module-import edge
    // runtimes) have no relative-URL-fetchable `import.meta.url` for the
    // default `__wbg_init()` fallback to resolve - they get the compiled
    // WebAssembly.Module directly via their own `.wasm` ES module import
    // instead. Checking a `globalThis` override here lets that host hand it
    // in (`globalThis.__matchboxWasmModule = wasmModule` before the first
    // exported call) without touching browser behavior at all: when the
    // override isn't set, this is exactly the old zero-arg call.
    bootstrap.push_str("            await __wbg_init(typeof globalThis !== \"undefined\" && globalThis.__matchboxWasmModule !== undefined ? { module_or_path: globalThis.__matchboxWasmModule } : undefined);\n");
    bootstrap.push_str("            if (!vm) {\n");
    bootstrap.push_str("                vm = new BoxLangVM();\n");
    bootstrap.push_str(r#"                if (typeof window !== "undefined" && window.MatchBox && window.MatchBox.registerHost) {
                    const vmPtr = vm.vm_ptr();
                    const hostId = window.MatchBox.registerHost({
                        invoke: (callbackId, thisVal, args) => _matchbox_invoke_callback(vmPtr, callbackId, thisVal, args),
                        pump: () => vm.pump(),
                        getProp: (gcId, name) => _matchbox_get_instance_prop(vmPtr, gcId, name),
                        getKeys: (gcId) => _matchbox_get_instance_keys(vmPtr, gcId),
                        setProp: (gcId, name, value) => _matchbox_set_instance_prop(vmPtr, gcId, name, value),
                    });
                    vm.__matchbox_host_id = hostId;
                    vm.set_host_id(hostId);
                }
"#);
    bootstrap.push_str("                try {\n");
    bootstrap.push_str("                    vm.init();\n");
    bootstrap.push_str("                } catch (initError) {\n");
    bootstrap
        .push_str("                    if (typeof console !== \"undefined\" && console.error) {\n");
    bootstrap.push_str(
        "                        console.error(\"[matchbox] VM init failed:\", initError);\n",
    );
    bootstrap.push_str("                    }\n");
    bootstrap.push_str("                    throw initError;\n");
    bootstrap.push_str("                }\n");
    bootstrap.push_str("            }\n");
    bootstrap.push_str("            return vm;\n");
    bootstrap.push_str("        })();\n");
    bootstrap.push_str("    }\n");
    bootstrap.push_str("    return await __matchboxReady;\n");
    bootstrap.push_str("}\n\n");

    for func in functions {
        bootstrap.push_str(&format!("export async function {}(...args) {{\n", func));
        bootstrap.push_str("    const vm = await ensureInit();\n");
        bootstrap.push_str("    try {\n");
        bootstrap.push_str(&format!(
            "        return await vm.call(\"{}\", args);\n",
            func
        ));
        bootstrap.push_str("    } catch (callError) {\n");
        bootstrap.push_str("        if (typeof console !== \"undefined\" && console.error) {\n");
        bootstrap.push_str(&format!(
            "            console.error(\"[matchbox] VM call to '{}' failed:\", callError);\n",
            func
        ));
        bootstrap.push_str("        }\n");
        bootstrap.push_str("        throw callError;\n");
        bootstrap.push_str("    }\n");
        bootstrap.push_str("}\n\n");
    }

    bootstrap.push_str("if (typeof window !== \"undefined\") {\n");
    bootstrap.push_str("    window.MatchBox = window.MatchBox || {};\n");
    bootstrap.push_str("    window.MatchBox.runtime = window.MatchBox.runtime || \"browser\";\n");
    bootstrap
        .push_str("    window.MatchBox.contractVersion = window.MatchBox.contractVersion || 1;\n");
    bootstrap.push_str("    window.MatchBox.modules = window.MatchBox.modules || {};\n");
    bootstrap
        .push_str("    window.MatchBox._readySignals = window.MatchBox._readySignals || {};\n");
    bootstrap.push_str("    window.MatchBox.ready = window.MatchBox.ready || function(stem) {\n");
    bootstrap
        .push_str("        return window.MatchBox._readySignals[stem] || Promise.resolve();\n");
    bootstrap.push_str("    };\n");
    bootstrap.push_str("    window.MatchBox.createModuleState = window.MatchBox.createModuleState || createModuleState;\n");
    bootstrap.push_str("    window.MatchBox.State = window.MatchBox.State || createModuleState;\n");
    bootstrap.push_str(&format!(
        "    window.MatchBox.modules[\"{}\"] = {{\n",
        module_name
    ));
    for func in functions {
        bootstrap.push_str(&format!("        {},\n", func));
    }
    bootstrap.push_str("    };\n");
    bootstrap.push_str("}\n\n");

    bootstrap.push_str("export const ready = ensureInit();\n\n");

    bootstrap.push_str("if (typeof window !== \"undefined\") {\n");
    bootstrap.push_str(&format!(
        "    window.MatchBox._readySignals[\"{}\"] = ready;\n",
        module_name
    ));
    bootstrap.push_str("    ready.then(() => {\n");
    bootstrap.push_str("        if (typeof window.dispatchEvent === \"function\") {\n");
    bootstrap.push_str("            window.dispatchEvent(new CustomEvent(\"matchbox:ready\", {\n");
    bootstrap.push_str(&format!(
        "                detail: {{ module: \"{}\" }}\n",
        module_name
    ));
    bootstrap.push_str("            }));\n");
    bootstrap.push_str("        }\n");
    bootstrap.push_str("    });\n");
    bootstrap.push_str("}\n");

    bootstrap
}

/// Generates bootstrap JS for the stub-based (no-Cargo) build path.
/// Same callback bridge and MatchBox infrastructure as the fusion bootstrap,
/// but uses `load_bytecode()` with base64-encoded bytecode instead of `init()`.
pub fn render_stub_js_bootstrap(
    functions: &[String],
    module_name: &str,
    b64_bytecode: &str,
) -> String {
    let mut bootstrap = String::new();

    // Base64 bytecode constant
    bootstrap.push_str(&format!(
        "const __matchboxBytecodeBase64 = \"{}\";\n\n",
        b64_bytecode
    ));

    // Callback, pump, and instance bridge setup
    bootstrap.push_str(r#"if (typeof window !== "undefined") {
    window.MatchBox = window.MatchBox || {};
    window.MatchBox._hosts = window.MatchBox._hosts || new Map();
    window.MatchBox._nextHostId = window.MatchBox._nextHostId || 1;
    window.MatchBox.registerHost = window.MatchBox.registerHost || function(host) {
        const hostId = window.MatchBox._nextHostId++;
        window.MatchBox._hosts.set(hostId, host);
        return hostId;
    };
    window.MatchBox.unregisterHost = window.MatchBox.unregisterHost || function(hostId) {
        window.MatchBox._hosts.delete(hostId);
    };
    window.MatchBox._host = window.MatchBox._host || function(hostId) {
        const host = window.MatchBox._hosts.get(hostId);
        if (!host) {
            throw new Error("MatchBox host is not registered: " + hostId);
        }
        return host;
    };
    window.MatchBox.invokeCallback = window.MatchBox.invokeCallback || function(hostId, callbackId, thisVal, args) {
        return window.MatchBox._host(hostId).invoke(callbackId, thisVal, args);
    };
    window.MatchBox.schedulePump = window.MatchBox.schedulePump || function(hostId) {
        if (!window.MatchBox._hosts.has(hostId)) {
            return;
        }
        const pump = () => {
            const host = window.MatchBox._hosts.get(hostId);
            if (host) {
                try {
                    host.pump();
                } catch (error) {
                    if (typeof console !== "undefined" && console.error) {
                        console.error("MatchBox scheduled pump failed", error);
                    }
                }
            }
        };
        if (typeof queueMicrotask === "function") {
            queueMicrotask(pump);
        } else {
            setTimeout(pump, 0);
        }
    };
    window.MatchBox.getInstanceProperty = window.MatchBox.getInstanceProperty || function(hostId, gcId, name) {
        return window.MatchBox._host(hostId).getProp(gcId, name);
    };
    window.MatchBox.getInstanceKeys = window.MatchBox.getInstanceKeys || function(hostId, gcId) {
        return window.MatchBox._host(hostId).getKeys(gcId);
    };
    window.MatchBox.setInstanceProperty = window.MatchBox.setInstanceProperty || function(hostId, gcId, name, value) {
        window.MatchBox._host(hostId).setProp(gcId, name, value);
    };
"#);
    bootstrap.push_str("    window.MatchBox.__matchbox_proxy_targets = window.MatchBox.__matchbox_proxy_targets || new WeakMap();\n");
    bootstrap.push_str(r#"    window.MatchBox.wrapInstancePropertyValue = window.MatchBox.wrapInstancePropertyValue || function(hostId, gcId, prop, value, ownerReceiver) {
        if (value == null || typeof value !== "object") return value;
        if (ArrayBuffer.isView(value) || value instanceof ArrayBuffer || value instanceof Date || value instanceof RegExp || value instanceof Promise) {
            return value;
        }
        if (value.__matchbox_nested_proxy__) return value;
        const persist = () => {
            if (ownerReceiver != null && typeof ownerReceiver === "object") {
                Reflect.set(ownerReceiver, prop, value, ownerReceiver);
            } else {
                window.MatchBox.setInstanceProperty(hostId, gcId, prop, value);
            }
        };
        const wrapChild = child => window.MatchBox.wrapInstancePropertyValue(hostId, gcId, prop, child, ownerReceiver);
        for (const key of Reflect.ownKeys(value)) {
            const current = value[key];
            if (current != null && typeof current === "object") {
                value[key] = wrapChild(current);
            }
        }
        const proxy = new Proxy(value, {
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
        window.MatchBox.__matchbox_proxy_targets.set(proxy, value);
        return proxy;
    };
"#);
    bootstrap.push_str("    window.MatchBox.createInstanceProxy = window.MatchBox.createInstanceProxy || function(hostId, gcId) {\n");
    bootstrap.push_str("        const target = { __matchbox_host_id: hostId, __matchbox_gc_id: gcId, __matchbox_cache: {} };\n");
    bootstrap.push_str("        return new Proxy(target, {\n");
    bootstrap.push_str("            get(target, prop, receiver) {\n");
    bootstrap.push_str("                if (typeof prop !== \"string\") return target[prop];\n");
    bootstrap
        .push_str("                if (prop.startsWith(\"__matchbox_\")) return target[prop];\n");
    bootstrap.push_str("                if (target.__matchbox_cache[prop]) return target.__matchbox_cache[prop];\n");
    bootstrap.push_str(
        "                let val = window.MatchBox.getInstanceProperty(hostId, gcId, prop);\n",
    );
    bootstrap.push_str("                if (val === undefined) {\n");
    bootstrap.push_str(
        "                    const keys = window.MatchBox.getInstanceKeys(hostId, gcId);\n",
    );
    bootstrap.push_str("                    const lowerProp = prop.toLowerCase();\n");
    bootstrap.push_str(
        "                    const matchedKey = keys.find(k => k.toLowerCase() === lowerProp);\n",
    );
    bootstrap.push_str("                    if (matchedKey && matchedKey !== prop) {\n");
    bootstrap.push_str("                        val = window.MatchBox.getInstanceProperty(hostId, gcId, matchedKey);\n");
    bootstrap.push_str("                    }\n");
    bootstrap.push_str("                }\n");
    bootstrap.push_str("                if (typeof val === \"function\") {\n");
    bootstrap.push_str(
        "                    // Keep methods unbound so `this` stays on the actual JS receiver.\n",
    );
    bootstrap.push_str(
        "                    // That lets reactive wrappers observe BoxLang instance writes.\n",
    );
    bootstrap.push_str("                    target.__matchbox_cache[prop] = val;\n");
    bootstrap.push_str("                } else if (val != null && typeof val === \"object\") {\n");
    bootstrap.push_str("                    val = window.MatchBox.wrapInstancePropertyValue(hostId, gcId, prop, val, receiver);\n");
    bootstrap.push_str("                }\n");
    bootstrap.push_str("                return val !== undefined ? val : target[prop];\n");
    bootstrap.push_str("            },\n");
    bootstrap.push_str("            set(target, prop, value) {\n");
    bootstrap.push_str("                if (typeof prop !== \"string\") return false;\n");
    bootstrap.push_str("                if (prop.startsWith(\"__matchbox_\")) { target[prop] = value; return true; }\n");
    bootstrap
        .push_str("                const keys = window.MatchBox.getInstanceKeys(hostId, gcId);\n");
    bootstrap.push_str("                const lowerProp = prop.toLowerCase();\n");
    bootstrap.push_str(
        "                const matchedKey = keys.find(k => k.toLowerCase() === lowerProp);\n",
    );
    bootstrap.push_str("                window.MatchBox.setInstanceProperty(hostId, gcId, matchedKey || prop, value);\n");
    bootstrap.push_str("                target[prop] = value;\n");
    bootstrap.push_str("                return true;\n");
    bootstrap.push_str("            },\n");
    bootstrap.push_str("            has(target, prop) {\n");
    bootstrap.push_str("                if (typeof prop !== \"string\") return prop in target;\n");
    bootstrap.push_str("                if (prop.startsWith(\"__matchbox_\")) return true;\n");
    bootstrap.push_str("                if (prop in target) return true;\n");
    bootstrap
        .push_str("                const keys = window.MatchBox.getInstanceKeys(hostId, gcId);\n");
    bootstrap.push_str("                const lowerProp = prop.toLowerCase();\n");
    bootstrap.push_str("                return keys.some(k => k.toLowerCase() === lowerProp);\n");
    bootstrap.push_str("            },\n");
    bootstrap.push_str("            ownKeys(target) {\n");
    bootstrap
        .push_str("                const bxKeys = window.MatchBox.getInstanceKeys(hostId, gcId);\n");
    bootstrap.push_str("                const targetKeys = Reflect.ownKeys(target);\n");
    bootstrap.push_str("                const allKeys = new Set([...targetKeys, ...bxKeys]);\n");
    bootstrap.push_str("                return Array.from(allKeys);\n");
    bootstrap.push_str("            },\n");
    bootstrap.push_str("            getOwnPropertyDescriptor(target, prop) {\n");
    bootstrap.push_str("                if (typeof prop !== \"string\") return Reflect.getOwnPropertyDescriptor(target, prop);\n");
    bootstrap.push_str("                if (prop.startsWith(\"__matchbox_\")) return Reflect.getOwnPropertyDescriptor(target, prop);\n");
    bootstrap
        .push_str("                const keys = window.MatchBox.getInstanceKeys(hostId, gcId);\n");
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
    bootstrap.push_str("            const ptr = this.__matchbox_host_id;\n");
    bootstrap.push_str("            try {\n");
    bootstrap.push_str("                return __matchboxOriginalFree.call(this);\n");
    bootstrap.push_str("            } finally {\n");
    bootstrap.push_str("                if (ptr && window.MatchBox.unregisterHost) {\n");
    bootstrap.push_str("                    window.MatchBox.unregisterHost(ptr);\n");
    bootstrap.push_str("                }\n");
    bootstrap.push_str("            }\n");
    bootstrap.push_str("        };\n");
    bootstrap.push_str("        if (typeof Symbol !== \"undefined\" && Symbol.dispose) {\n");
    bootstrap
        .push_str("            BoxLangVM.prototype[Symbol.dispose] = BoxLangVM.prototype.free;\n");
    bootstrap.push_str("        }\n");
    bootstrap.push_str("    }\n");
    bootstrap.push_str("}\n\n");

    bootstrap.push_str("let vm = null;\n");
    bootstrap.push_str("let __matchboxReady = null;\n");
    bootstrap.push_str("function isPlainObject(value) {\n");
    bootstrap.push_str(
        "    return value != null && typeof value === \"object\" && !Array.isArray(value);\n",
    );
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
    bootstrap
        .push_str("    throw new Error(`MatchBox module ${moduleName} did not become ready`);\n");
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
    // See the sibling ensureInit() above for why this checks a globalThis
    // override before falling back to the zero-arg browser-fetch path.
    bootstrap.push_str("            await __wbg_init(typeof globalThis !== \"undefined\" && globalThis.__matchboxWasmModule !== undefined ? { module_or_path: globalThis.__matchboxWasmModule } : undefined);\n");
    bootstrap.push_str("            if (!vm) {\n");
    bootstrap.push_str("                vm = new BoxLangVM();\n");
    bootstrap.push_str(r#"                if (typeof window !== "undefined" && window.MatchBox && window.MatchBox.registerHost) {
                    const vmPtr = vm.vm_ptr();
                    const hostId = window.MatchBox.registerHost({
                        invoke: (callbackId, thisVal, args) => _matchbox_invoke_callback(vmPtr, callbackId, thisVal, args),
                        pump: () => vm.pump(),
                        getProp: (gcId, name) => _matchbox_get_instance_prop(vmPtr, gcId, name),
                        getKeys: (gcId) => _matchbox_get_instance_keys(vmPtr, gcId),
                        setProp: (gcId, name, value) => _matchbox_set_instance_prop(vmPtr, gcId, name, value),
                    });
                    vm.__matchbox_host_id = hostId;
                    vm.set_host_id(hostId);
                }
"#);
    bootstrap.push_str("                const bytecodeBytes = Uint8Array.from(atob(__matchboxBytecodeBase64), c => c.charCodeAt(0));\n");
    bootstrap.push_str("                try {\n");
    bootstrap.push_str("                    vm.load_bytecode(bytecodeBytes);\n");
    bootstrap.push_str("                } catch (loadError) {\n");
    bootstrap
        .push_str("                    if (typeof console !== \"undefined\" && console.error) {\n");
    bootstrap.push_str("                        console.error(\"[matchbox] VM load_bytecode failed:\", loadError);\n");
    bootstrap.push_str("                    }\n");
    bootstrap.push_str("                    throw loadError;\n");
    bootstrap.push_str("                }\n");
    bootstrap.push_str("            }\n");
    bootstrap.push_str("            return vm;\n");
    bootstrap.push_str("        })();\n");
    bootstrap.push_str("    }\n");
    bootstrap.push_str("    return await __matchboxReady;\n");
    bootstrap.push_str("}\n\n");

    for func in functions {
        bootstrap.push_str(&format!("export async function {}(...args) {{\n", func));
        bootstrap.push_str("    const vm = await ensureInit();\n");
        bootstrap.push_str("    try {\n");
        bootstrap.push_str(&format!(
            "        return await vm.call(\"{}\", args);\n",
            func
        ));
        bootstrap.push_str("    } catch (callError) {\n");
        bootstrap.push_str("        if (typeof console !== \"undefined\" && console.error) {\n");
        bootstrap.push_str(&format!(
            "            console.error(\"[matchbox] VM call to '{}' failed:\", callError);\n",
            func
        ));
        bootstrap.push_str("        }\n");
        bootstrap.push_str("        throw callError;\n");
        bootstrap.push_str("    }\n");
        bootstrap.push_str("}\n\n");
    }

    bootstrap.push_str("if (typeof window !== \"undefined\") {\n");
    bootstrap.push_str("    window.MatchBox = window.MatchBox || {};\n");
    bootstrap.push_str("    window.MatchBox.runtime = window.MatchBox.runtime || \"browser\";\n");
    bootstrap
        .push_str("    window.MatchBox.contractVersion = window.MatchBox.contractVersion || 1;\n");
    bootstrap.push_str("    window.MatchBox.modules = window.MatchBox.modules || {};\n");
    bootstrap
        .push_str("    window.MatchBox._readySignals = window.MatchBox._readySignals || {};\n");
    bootstrap.push_str("    window.MatchBox.ready = window.MatchBox.ready || function(stem) {\n");
    bootstrap
        .push_str("        return window.MatchBox._readySignals[stem] || Promise.resolve();\n");
    bootstrap.push_str("    };\n");
    bootstrap.push_str("    window.MatchBox.createModuleState = window.MatchBox.createModuleState || createModuleState;\n");
    bootstrap.push_str("    window.MatchBox.State = window.MatchBox.State || createModuleState;\n");
    bootstrap.push_str(&format!(
        "    window.MatchBox.modules[\"{}\"] = {{\n",
        module_name
    ));
    for func in functions {
        bootstrap.push_str(&format!("        {},\n", func));
    }
    bootstrap.push_str("    };\n");
    bootstrap.push_str("}\n\n");

    bootstrap.push_str("export const ready = ensureInit();\n\n");

    bootstrap.push_str("if (typeof window !== \"undefined\") {\n");
    bootstrap.push_str(&format!(
        "    window.MatchBox._readySignals[\"{}\"] = ready;\n",
        module_name
    ));
    bootstrap.push_str("    ready.then(() => {\n");
    bootstrap.push_str("        if (typeof window.dispatchEvent === \"function\") {\n");
    bootstrap.push_str("            window.dispatchEvent(new CustomEvent(\"matchbox:ready\", {\n");
    bootstrap.push_str(&format!(
        "                detail: {{ module: \"{}\" }}\n",
        module_name
    ));
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
        assert_eq!(
            exported_function_names(&ast),
            vec!["alpha".to_string(), "beta".to_string()]
        );
    }

    #[test]
    fn pure_js_bootstrap_keeps_stub_loader_shape() {
        let bootstrap =
            render_pure_js_bootstrap(&vec!["hello".to_string()], "stub-wasm", "stub-bytecode");

        assert!(bootstrap.contains("const wasmBase64 = \"stub-wasm\";"));
        assert!(bootstrap.contains("const bytecodeBase64 = \"stub-bytecode\";"));
        assert!(bootstrap.contains("vm.load_bytecode(bytecodeBinary);"));
        assert!(bootstrap.contains("return await vm.call(\"hello\", args);"));
        assert!(bootstrap.contains("if (e instanceof Error) throw e;"));
    }

    #[test]
    fn fusion_js_bootstrap_uses_async_vm_call_without_bytecode_loader() {
        let bootstrap = render_fusion_js_bootstrap(&vec!["hello".to_string()], "app");

        assert!(bootstrap.contains("registerHost"));
        assert!(bootstrap.contains("invokeCallback"));
        assert!(bootstrap.contains("BoxLangVM.prototype.free"));
        assert!(bootstrap.contains(
            "await __wbg_init(typeof globalThis !== \"undefined\" && globalThis.__matchboxWasmModule !== undefined ? { module_or_path: globalThis.__matchboxWasmModule } : undefined);"
        ));
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
        assert!(bootstrap.contains("registerHost"));
        assert!(bootstrap.contains("vm.set_host_id(hostId)"));
        assert!(bootstrap.contains("invokeCallback"));
        assert!(bootstrap.contains("schedulePump"));
        assert!(bootstrap.contains("createInstanceProxy"));
        assert!(bootstrap.contains("getInstanceProperty"));
        assert!(bootstrap.contains("getInstanceKeys"));
        assert!(bootstrap.contains("setInstanceProperty"));
        assert!(bootstrap.contains("wrapInstancePropertyValue"));
        assert!(bootstrap.contains("globalThis.__matchboxWasmModule"));
        assert!(bootstrap.contains("vm = new BoxLangVM();"));
        assert!(bootstrap.contains("vm.load_bytecode(bytecodeBytes);"));
        assert!(bootstrap.contains("return await vm.call(\"hello\", args);"));
        assert!(bootstrap.contains("window.MatchBox.modules[\"app\"]"));
        assert!(bootstrap.contains("hello,"));
        assert!(!bootstrap.contains("vm.init();"));
    }
}
