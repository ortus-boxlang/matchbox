/* STANDALONE MATCHBOX JS BUNDLE TEMPLATE (WASI BASED) */

let wasm;
const cachedTextDecoder = new TextDecoder('utf-8');
const cachedTextEncoder = new TextEncoder();

// Minimal WASI shim
const wasiShim = {
    fd_write: (fd, iovs, iovs_len, nwritten) => {
        const view = new DataView(wasm.memory.buffer);
        let written = 0;
        for (let i = 0; i < iovs_len; i++) {
            const ptr = view.getUint32(iovs + i * 8, true);
            const len = view.getUint32(iovs + i * 8 + 4, true);
            const str = cachedTextDecoder.decode(new Uint8Array(wasm.memory.buffer).subarray(ptr, ptr + len));
            if (fd === 1 || fd === 2) console.log(str);
            written += len;
        }
        view.setUint32(nwritten, written, true);
        return 0;
    },
    random_get: (buf, len) => {
        crypto.getRandomValues(new Uint8Array(wasm.memory.buffer).subarray(buf, buf + len));
        return 0;
    },
    proc_exit: (code) => { throw new Error("Process exited with code " + code); },
    environ_sizes_get: (count, buf_size) => {
        const view = new DataView(wasm.memory.buffer);
        view.setUint32(count, 0, true);
        view.setUint32(buf_size, 0, true);
        return 0;
    },
    environ_get: (environ, environ_buf) => 0,
    args_sizes_get: (count, buf_size) => {
        const view = new DataView(wasm.memory.buffer);
        view.setUint32(count, 0, true);
        view.setUint32(buf_size, 0, true);
        return 0;
    },
    args_get: (args, args_buf) => 0,
    clock_time_get: (id, precision, time) => {
        const view = new DataView(wasm.memory.buffer);
        const now = BigInt(Date.now()) * 1000000n;
        view.setBigUint64(time, now, true);
        return 0;
    },
    poll_oneoff: (in_ptr, out_ptr, nsubscriptions, nevents) => {
        return 0;
    },
    fd_close: (fd) => 0,
    fd_read: (fd, iovs, iovs_len, nread) => 0,
    fd_seek: (fd, offset, whence, newoffset) => 0,
    fd_fdstat_get: (fd, stat) => 0,
    fd_filestat_get: (fd, stat) => 0,
    fd_prestat_get: (fd, prestat) => 0,
    fd_prestat_dir_name: (fd, path, path_len) => 0,
    fd_readdir: (fd, buf, buf_len, cookie, buf_used) => {
        const view = new DataView(wasm.memory.buffer);
        view.setUint32(buf_used, 0, true);
        return 0;
    },
    path_open: (fd, dirflags, path, path_len, oflags, fs_rights_base, fs_rights_inheriting, fdflags, opened_fd) => 0,
    path_filestat_get: (fd, flags, path, path_len, stat) => 0,
    path_remove_directory: (fd, path, path_len) => 0,
    path_unlink_file: (fd, path, path_len) => 0
};

async function init(wasmBytes) {
    const imports = {
        wasi_snapshot_preview1: wasiShim,
        matchbox_js_host: matchbox_js_host
    };

    const { instance } = await WebAssembly.instantiate(wasmBytes, imports);
    wasm = instance.exports;
}

const jsHandles = new Map([[1, globalThis]]);
let nextJsHandle = 2;

function bxEncodeResult(val, strBufPtr, strBufLen, outStrLenPtr, outNumPtr, outBoolPtr, outObjPtr) {
    const mem = new DataView(wasm.memory.buffer);
    const u8 = new Uint8Array(wasm.memory.buffer);
    if (val === null || val === undefined) return 0;
    if (typeof val === 'boolean') { mem.setInt32(outBoolPtr, val ? 1 : 0, true); return 1; }
    if (typeof val === 'number') { mem.setFloat64(outNumPtr, val, true); return 2; }
    if (typeof val === 'string') {
        const encoded = cachedTextEncoder.encode(val);
        const writeLen = Math.min(encoded.length, strBufLen);
        u8.set(encoded.subarray(0, writeLen), strBufPtr);
        mem.setUint32(outStrLenPtr, encoded.length, true);
        return 3;
    }
    if (typeof val === 'object' || typeof val === 'function') {
        const id = nextJsHandle++;
        jsHandles.set(id, val);
        mem.setUint32(outObjPtr, id, true);
        return 4;
    }
    return 0;
}

const matchbox_js_host = {
    bx_js_get_prop: (objId, keyPtr, keyLen, strBufPtr, strBufLen, outStrLenPtr, outNumPtr, outBoolPtr, outObjPtr) => {
        const obj = jsHandles.get(objId);
        if (obj == null) return 0;
        const key = cachedTextDecoder.decode(new Uint8Array(wasm.memory.buffer).subarray(keyPtr, keyPtr + keyLen));
        return bxEncodeResult(obj[key], strBufPtr, strBufLen, outStrLenPtr, outNumPtr, outBoolPtr, outObjPtr);
    },
    bx_js_set_prop_null: (objId, keyPtr, keyLen) => {
        const obj = jsHandles.get(objId); if (!obj) return;
        const key = cachedTextDecoder.decode(new Uint8Array(wasm.memory.buffer).subarray(keyPtr, keyPtr + keyLen));
        obj[key] = null;
    },
    bx_js_set_prop_bool: (objId, keyPtr, keyLen, val) => {
        const obj = jsHandles.get(objId); if (!obj) return;
        const key = cachedTextDecoder.decode(new Uint8Array(wasm.memory.buffer).subarray(keyPtr, keyPtr + keyLen));
        obj[key] = val !== 0;
    },
    bx_js_set_prop_num: (objId, keyPtr, keyLen, val) => {
        const obj = jsHandles.get(objId); if (!obj) return;
        const key = cachedTextDecoder.decode(new Uint8Array(wasm.memory.buffer).subarray(keyPtr, keyPtr + keyLen));
        obj[key] = val;
    },
    bx_js_set_prop_str: (objId, keyPtr, keyLen, valPtr, valLen) => {
        const obj = jsHandles.get(objId); if (!obj) return;
        const u8 = new Uint8Array(wasm.memory.buffer);
        const key = cachedTextDecoder.decode(u8.subarray(keyPtr, keyPtr + keyLen));
        obj[key] = cachedTextDecoder.decode(u8.subarray(valPtr, valPtr + valLen));
    },
    bx_js_set_prop_obj: (objId, keyPtr, keyLen, valId) => {
        const obj = jsHandles.get(objId); if (!obj) return;
        const key = cachedTextDecoder.decode(new Uint8Array(wasm.memory.buffer).subarray(keyPtr, keyPtr + keyLen));
        obj[key] = jsHandles.get(valId) ?? null;
    },
    bx_js_call_method: (objId, methodPtr, methodLen, argsJsonPtr, argsJsonLen, strBufPtr, strBufLen, outStrLenPtr, outNumPtr, outBoolPtr, outObjPtr) => {
        const obj = jsHandles.get(objId);
        if (obj == null) return 0;
        const u8 = new Uint8Array(wasm.memory.buffer);
        const method = cachedTextDecoder.decode(u8.subarray(methodPtr, methodPtr + methodLen));
        const argsJson = cachedTextDecoder.decode(u8.subarray(argsJsonPtr, argsJsonPtr + argsJsonLen));
        let args = [];
        try {
            const raw = JSON.parse(argsJson);
            args = raw.map(a => (a && typeof a === 'object' && 'h' in a) ? (jsHandles.get(a.h) ?? null) : a);
        } catch (_) {}
        try {
            return bxEncodeResult(obj[method](...args), strBufPtr, strBufLen, outStrLenPtr, outNumPtr, outBoolPtr, outObjPtr);
        } catch (e) {
            console.error('[matchbox] JS call error: ' + method, e);
            return 0;
        }
    },
};

export class BoxLangVM {
    constructor() {}

    load_bytecode(bytes) {
        const len = bytes.length;
        const ptr = wasm.boxlang_alloc(len);
        const mem = new Uint8Array(wasm.memory.buffer);
        mem.set(bytes, ptr);
        const res = wasm.boxlang_load_bytecode(ptr, len);
        if (res !== 0) {
            // Try to retrieve the error message stored by the Rust side
            let detail = "code " + res;
            try {
                const errLen = wasm.boxlang_get_last_result_len();
                if (errLen > 0) {
                    const errPtr = wasm.boxlang_get_last_result_ptr?.() ?? 0;
                    const errBuf = new Uint8Array(wasm.memory.buffer).subarray(errPtr, errPtr + errLen);
                    const errStr = new TextDecoder().decode(errBuf);
                    const parsed = JSON.parse(errStr);
                    if (parsed && parsed.error) detail = parsed.error;
                }
            } catch (_) {}
            throw new Error("Failed to load bytecode: " + detail);
        }
    }

    async call(name, args) {
        const nameBuf = cachedTextEncoder.encode(name);
        const namePtr = wasm.boxlang_alloc(nameBuf.length);
        new Uint8Array(wasm.memory.buffer).set(nameBuf, namePtr);

        const argsBuf = cachedTextEncoder.encode(JSON.stringify(args));
        const argsPtr = wasm.boxlang_alloc(argsBuf.length);
        new Uint8Array(wasm.memory.buffer).set(argsBuf, argsPtr);

        const resPtr = wasm.boxlang_call(namePtr, nameBuf.length, argsPtr, argsBuf.length);
        const resLen = wasm.boxlang_get_last_result_len();
        
        const resBuf = new Uint8Array(wasm.memory.buffer).subarray(resPtr, resPtr + resLen);
        const resStr = new TextDecoder().decode(resBuf);
        const res = JSON.parse(resStr);
        
        if (res && res.error) {
            throw new Error(res.error);
        }
        return res;
    }
}

/* __REPLACE_ME__ */
