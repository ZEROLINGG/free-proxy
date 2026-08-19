var __defProp = Object.defineProperty;
var __name = (target, value) => __defProp(target, "name", { value, configurable: true });

// build/index.js
import { WorkerEntrypoint as me } from "cloudflare:workers";
import Z from "./8ce956ec5ec0eb91d9c0019c2a29d88d9395ff14-index_bg.wasm";
var H = globalThis.__worker_init_state = { criticalError: false, instanceId: 0 };
var E = class {
  static {
    __name(this, "E");
  }
  __destroy_into_raw() {
    let e = this.__wbg_ptr;
    return this.__wbg_ptr = 0, ue.unregister(this), e;
  }
  free() {
    let e = this.__destroy_into_raw();
    c();
    try {
      r.__wbg_containerstartupoptions_free(e, 0);
    } catch (t) {
      o(t);
    }
  }
  get enableInternet() {
    if (this.__wbg_inst !== void 0 && this.__wbg_inst !== s) throw new Error("Invalid stale object from previous Wasm instance");
    let e;
    c();
    try {
      e = r.__wbg_get_containerstartupoptions_enableInternet(this.__wbg_ptr);
    } catch (t) {
      o(t);
    }
    return e === 16777215 ? void 0 : e !== 0;
  }
  get entrypoint() {
    if (this.__wbg_inst !== void 0 && this.__wbg_inst !== s) throw new Error("Invalid stale object from previous Wasm instance");
    let e;
    c();
    try {
      e = r.__wbg_get_containerstartupoptions_entrypoint(this.__wbg_ptr);
    } catch (_) {
      o(_);
    }
    var t = ge(e[0], e[1]);
    return r.__wbindgen_free(e[0], e[1] * 4, 4), t;
  }
  get env() {
    if (this.__wbg_inst !== void 0 && this.__wbg_inst !== s) throw new Error("Invalid stale object from previous Wasm instance");
    let e;
    c();
    try {
      e = r.__wbg_get_containerstartupoptions_env(this.__wbg_ptr);
    } catch (t) {
      o(t);
    }
    return e;
  }
  set enableInternet(e) {
    if (this.__wbg_inst !== void 0 && this.__wbg_inst !== s) throw new Error("Invalid stale object from previous Wasm instance");
    c();
    try {
      r.__wbg_set_containerstartupoptions_enableInternet(this.__wbg_ptr, u(e) ? 16777215 : e ? 1 : 0);
    } catch (t) {
      o(t);
    }
  }
  set entrypoint(e) {
    if (this.__wbg_inst !== void 0 && this.__wbg_inst !== s) throw new Error("Invalid stale object from previous Wasm instance");
    let t = le(e, r.__wbindgen_malloc), _ = l;
    c();
    try {
      r.__wbg_set_containerstartupoptions_entrypoint(this.__wbg_ptr, t, _);
    } catch (i) {
      o(i);
    }
  }
  set env(e) {
    if (this.__wbg_inst !== void 0 && this.__wbg_inst !== s) throw new Error("Invalid stale object from previous Wasm instance");
    c();
    try {
      r.__wbg_set_containerstartupoptions_env(this.__wbg_ptr, e);
    } catch (t) {
      o(t);
    }
  }
};
Symbol.dispose && (E.prototype[Symbol.dispose] = E.prototype.free);
var R = class {
  static {
    __name(this, "R");
  }
  __destroy_into_raw() {
    let e = this.__wbg_ptr;
    return this.__wbg_ptr = 0, be.unregister(this), e;
  }
  free() {
    let e = this.__destroy_into_raw();
    c();
    try {
      r.__wbg_intounderlyingbytesource_free(e, 0);
    } catch (t) {
      o(t);
    }
  }
  get autoAllocateChunkSize() {
    if (this.__wbg_inst !== void 0 && this.__wbg_inst !== s) throw new Error("Invalid stale object from previous Wasm instance");
    let e;
    c();
    try {
      e = r.intounderlyingbytesource_autoAllocateChunkSize(this.__wbg_ptr);
    } catch (t) {
      o(t);
    }
    return e >>> 0;
  }
  cancel() {
    if (this.__wbg_inst !== void 0 && this.__wbg_inst !== s) throw new Error("Invalid stale object from previous Wasm instance");
    let e = this.__destroy_into_raw();
    c();
    try {
      r.intounderlyingbytesource_cancel(e);
    } catch (t) {
      o(t);
    }
  }
  pull(e) {
    if (this.__wbg_inst !== void 0 && this.__wbg_inst !== s) throw new Error("Invalid stale object from previous Wasm instance");
    let t;
    c();
    try {
      t = r.intounderlyingbytesource_pull(this.__wbg_ptr, e);
    } catch (_) {
      o(_);
    }
    return t;
  }
  start(e) {
    if (this.__wbg_inst !== void 0 && this.__wbg_inst !== s) throw new Error("Invalid stale object from previous Wasm instance");
    c();
    try {
      r.intounderlyingbytesource_start(this.__wbg_ptr, e);
    } catch (t) {
      o(t);
    }
  }
  get type() {
    if (this.__wbg_inst !== void 0 && this.__wbg_inst !== s) throw new Error("Invalid stale object from previous Wasm instance");
    let e;
    c();
    try {
      e = r.intounderlyingbytesource_type(this.__wbg_ptr);
    } catch (t) {
      o(t);
    }
    return ae[e];
  }
};
Symbol.dispose && (R.prototype[Symbol.dispose] = R.prototype.free);
var j = class {
  static {
    __name(this, "j");
  }
  __destroy_into_raw() {
    let e = this.__wbg_ptr;
    return this.__wbg_ptr = 0, de.unregister(this), e;
  }
  free() {
    let e = this.__destroy_into_raw();
    c();
    try {
      r.__wbg_intounderlyingsink_free(e, 0);
    } catch (t) {
      o(t);
    }
  }
  abort(e) {
    if (this.__wbg_inst !== void 0 && this.__wbg_inst !== s) throw new Error("Invalid stale object from previous Wasm instance");
    let t = this.__destroy_into_raw(), _;
    c();
    try {
      _ = r.intounderlyingsink_abort(t, e);
    } catch (i) {
      o(i);
    }
    return _;
  }
  close() {
    if (this.__wbg_inst !== void 0 && this.__wbg_inst !== s) throw new Error("Invalid stale object from previous Wasm instance");
    let e = this.__destroy_into_raw(), t;
    c();
    try {
      t = r.intounderlyingsink_close(e);
    } catch (_) {
      o(_);
    }
    return t;
  }
  write(e) {
    if (this.__wbg_inst !== void 0 && this.__wbg_inst !== s) throw new Error("Invalid stale object from previous Wasm instance");
    let t;
    c();
    try {
      t = r.intounderlyingsink_write(this.__wbg_ptr, e);
    } catch (_) {
      o(_);
    }
    return t;
  }
};
Symbol.dispose && (j.prototype[Symbol.dispose] = j.prototype.free);
var m = class n {
  static {
    __name(this, "n");
  }
  static __wrap(e) {
    let t = Object.create(n.prototype);
    return t.__wbg_ptr = e, Object.defineProperty(t, "__wbg_inst", { value: s, writable: true }), K.register(t, { ptr: e, instance: s }, t), t;
  }
  __destroy_into_raw() {
    let e = this.__wbg_ptr;
    return this.__wbg_ptr = 0, K.unregister(this), e;
  }
  free() {
    let e = this.__destroy_into_raw();
    c();
    try {
      r.__wbg_intounderlyingsource_free(e, 0);
    } catch (t) {
      o(t);
    }
  }
  cancel() {
    if (this.__wbg_inst !== void 0 && this.__wbg_inst !== s) throw new Error("Invalid stale object from previous Wasm instance");
    let e = this.__destroy_into_raw();
    c();
    try {
      r.intounderlyingsource_cancel(e);
    } catch (t) {
      o(t);
    }
  }
  pull(e) {
    if (this.__wbg_inst !== void 0 && this.__wbg_inst !== s) throw new Error("Invalid stale object from previous Wasm instance");
    let t;
    c();
    try {
      t = r.intounderlyingsource_pull(this.__wbg_ptr, e);
    } catch (_) {
      o(_);
    }
    return t;
  }
};
Symbol.dispose && (m.prototype[Symbol.dispose] = m.prototype.free);
var v = class n2 {
  static {
    __name(this, "n");
  }
  static __wrap(e) {
    let t = Object.create(n2.prototype);
    return t.__wbg_ptr = e, Object.defineProperty(t, "__wbg_inst", { value: s, writable: true }), Q.register(t, { ptr: e, instance: s }, t), t;
  }
  __destroy_into_raw() {
    let e = this.__wbg_ptr;
    return this.__wbg_ptr = 0, Q.unregister(this), e;
  }
  free() {
    let e = this.__destroy_into_raw();
    c();
    try {
      r.__wbg_minifyconfig_free(e, 0);
    } catch (t) {
      o(t);
    }
  }
  get css() {
    if (this.__wbg_inst !== void 0 && this.__wbg_inst !== s) throw new Error("Invalid stale object from previous Wasm instance");
    let e;
    c();
    try {
      e = r.__wbg_get_minifyconfig_css(this.__wbg_ptr);
    } catch (t) {
      o(t);
    }
    return e !== 0;
  }
  get html() {
    if (this.__wbg_inst !== void 0 && this.__wbg_inst !== s) throw new Error("Invalid stale object from previous Wasm instance");
    let e;
    c();
    try {
      e = r.__wbg_get_minifyconfig_html(this.__wbg_ptr);
    } catch (t) {
      o(t);
    }
    return e !== 0;
  }
  get js() {
    if (this.__wbg_inst !== void 0 && this.__wbg_inst !== s) throw new Error("Invalid stale object from previous Wasm instance");
    let e;
    c();
    try {
      e = r.__wbg_get_minifyconfig_js(this.__wbg_ptr);
    } catch (t) {
      o(t);
    }
    return e !== 0;
  }
  set css(e) {
    if (this.__wbg_inst !== void 0 && this.__wbg_inst !== s) throw new Error("Invalid stale object from previous Wasm instance");
    c();
    try {
      r.__wbg_set_minifyconfig_css(this.__wbg_ptr, e);
    } catch (t) {
      o(t);
    }
  }
  set html(e) {
    if (this.__wbg_inst !== void 0 && this.__wbg_inst !== s) throw new Error("Invalid stale object from previous Wasm instance");
    c();
    try {
      r.__wbg_set_minifyconfig_html(this.__wbg_ptr, e);
    } catch (t) {
      o(t);
    }
  }
  set js(e) {
    if (this.__wbg_inst !== void 0 && this.__wbg_inst !== s) throw new Error("Invalid stale object from previous Wasm instance");
    c();
    try {
      r.__wbg_set_minifyconfig_js(this.__wbg_ptr, e);
    } catch (t) {
      o(t);
    }
  }
};
Symbol.dispose && (v.prototype[Symbol.dispose] = v.prototype.free);
var F = class {
  static {
    __name(this, "F");
  }
  __destroy_into_raw() {
    let e = this.__wbg_ptr;
    return this.__wbg_ptr = 0, we.unregister(this), e;
  }
  free() {
    let e = this.__destroy_into_raw();
    c();
    try {
      r.__wbg_r2range_free(e, 0);
    } catch (t) {
      o(t);
    }
  }
  get length() {
    if (this.__wbg_inst !== void 0 && this.__wbg_inst !== s) throw new Error("Invalid stale object from previous Wasm instance");
    let e;
    c();
    try {
      e = r.__wbg_get_r2range_length(this.__wbg_ptr);
    } catch (t) {
      o(t);
    }
    return e[0] === 0 ? void 0 : e[1];
  }
  get offset() {
    if (this.__wbg_inst !== void 0 && this.__wbg_inst !== s) throw new Error("Invalid stale object from previous Wasm instance");
    let e;
    c();
    try {
      e = r.__wbg_get_r2range_offset(this.__wbg_ptr);
    } catch (t) {
      o(t);
    }
    return e[0] === 0 ? void 0 : e[1];
  }
  get suffix() {
    if (this.__wbg_inst !== void 0 && this.__wbg_inst !== s) throw new Error("Invalid stale object from previous Wasm instance");
    let e;
    c();
    try {
      e = r.__wbg_get_r2range_suffix(this.__wbg_ptr);
    } catch (t) {
      o(t);
    }
    return e[0] === 0 ? void 0 : e[1];
  }
  set length(e) {
    if (this.__wbg_inst !== void 0 && this.__wbg_inst !== s) throw new Error("Invalid stale object from previous Wasm instance");
    c();
    try {
      r.__wbg_set_r2range_length(this.__wbg_ptr, !u(e), u(e) ? 0 : e);
    } catch (t) {
      o(t);
    }
  }
  set offset(e) {
    if (this.__wbg_inst !== void 0 && this.__wbg_inst !== s) throw new Error("Invalid stale object from previous Wasm instance");
    c();
    try {
      r.__wbg_set_r2range_offset(this.__wbg_ptr, !u(e), u(e) ? 0 : e);
    } catch (t) {
      o(t);
    }
  }
  set suffix(e) {
    if (this.__wbg_inst !== void 0 && this.__wbg_inst !== s) throw new Error("Invalid stale object from previous Wasm instance");
    c();
    try {
      r.__wbg_set_r2range_suffix(this.__wbg_ptr, !u(e), u(e) ? 0 : e);
    } catch (t) {
      o(t);
    }
  }
};
Symbol.dispose && (F.prototype[Symbol.dispose] = F.prototype.free);
function T() {
  s++, y = null, W = null, k = null, typeof numBytesDecoded < "u" && (numBytesDecoded = 0), typeof l < "u" && (l = 0), q = false, z = false, D = new WebAssembly.Instance(Z, _e()), r = D.exports, r.__wbindgen_start();
}
__name(T, "T");
function ee() {
  let n3;
  c();
  try {
    n3 = r.__worker_init_state();
  } catch (e) {
    o(e);
  }
  return n3;
}
__name(ee, "ee");
function te(n3, e, t) {
  let _;
  c();
  try {
    _ = r.fetch(n3, e, t);
  } catch (i) {
    o(i);
  }
  return _;
}
__name(te, "te");
function ne() {
  c();
  try {
    r.init();
  } catch (n3) {
    o(n3);
  }
}
__name(ne, "ne");
function _e() {
  return { __proto__: null, "./index_bg.js": { __proto__: null, __wbg_Error_408e67f47ca7b58b: /* @__PURE__ */ __name(function(e, t) {
    return Error(h(e, t));
  }, "__wbg_Error_408e67f47ca7b58b"), __wbg_String_8564e559799eccda: /* @__PURE__ */ __name(function(e, t) {
    let _ = String(t), i = I(_, r.__wbindgen_malloc, r.__wbindgen_realloc), a = l;
    d().setInt32(e + 4, a, true), d().setInt32(e + 0, i, true);
  }, "__wbg_String_8564e559799eccda"), __wbg___wbindgen_debug_string_a57024b9c6e4a48b: /* @__PURE__ */ __name(function(e, t) {
    let _ = J(t), i = I(_, r.__wbindgen_malloc, r.__wbindgen_realloc), a = l;
    d().setInt32(e + 4, a, true), d().setInt32(e + 0, i, true);
  }, "__wbg___wbindgen_debug_string_a57024b9c6e4a48b"), __wbg___wbindgen_is_function_5e4570eb24ffa122: /* @__PURE__ */ __name(function(e) {
    return typeof e == "function";
  }, "__wbg___wbindgen_is_function_5e4570eb24ffa122"), __wbg___wbindgen_is_null_or_undefined_d3f0c1e48e6f1b85: /* @__PURE__ */ __name(function(e) {
    return e == null;
  }, "__wbg___wbindgen_is_null_or_undefined_d3f0c1e48e6f1b85"), __wbg___wbindgen_is_string_e6f02f0ea5f20a32: /* @__PURE__ */ __name(function(e) {
    return typeof e == "string";
  }, "__wbg___wbindgen_is_string_e6f02f0ea5f20a32"), __wbg___wbindgen_is_undefined_6cff064c44e0d823: /* @__PURE__ */ __name(function(e) {
    return e === void 0;
  }, "__wbg___wbindgen_is_undefined_6cff064c44e0d823"), __wbg___wbindgen_reinit_eaa1836ea9a8a649: /* @__PURE__ */ __name(function() {
    z = true;
  }, "__wbg___wbindgen_reinit_eaa1836ea9a8a649"), __wbg___wbindgen_string_get_d154f1e671052120: /* @__PURE__ */ __name(function(e, t) {
    let _ = t, i = typeof _ == "string" ? _ : void 0;
    var a = u(i) ? 0 : I(i, r.__wbindgen_malloc, r.__wbindgen_realloc), f = l;
    d().setInt32(e + 4, f, true), d().setInt32(e + 0, a, true);
  }, "__wbg___wbindgen_string_get_d154f1e671052120"), __wbg___wbindgen_throw_bb96b2010945f0bc: /* @__PURE__ */ __name(function(e, t) {
    throw new WebAssembly.Exception(V, [new Error(h(e, t))]);
  }, "__wbg___wbindgen_throw_bb96b2010945f0bc"), __wbg__wbg_cb_unref_be22cc64ae6946a0: /* @__PURE__ */ __name(function(e) {
    e._wbg_cb_unref();
  }, "__wbg__wbg_cb_unref_be22cc64ae6946a0"), __wbg_append_acad6a3f39a3e778: /* @__PURE__ */ __name(function(e, t, _, i, a) {
    e.append(h(t, _), h(i, a));
  }, "__wbg_append_acad6a3f39a3e778"), __wbg_body_c5183699e6723ef9: /* @__PURE__ */ __name(function(e) {
    let t = e.body;
    return u(t) ? 0 : w(t);
  }, "__wbg_body_c5183699e6723ef9"), __wbg_body_eb2e7e7701fa47ae: /* @__PURE__ */ __name(function(e) {
    let t = e.body;
    return u(t) ? 0 : w(t);
  }, "__wbg_body_eb2e7e7701fa47ae"), __wbg_buffer_78291c0e094ccf99: /* @__PURE__ */ __name(function(e) {
    return e.buffer;
  }, "__wbg_buffer_78291c0e094ccf99"), __wbg_byobRequest_f8b1c89429b77545: /* @__PURE__ */ __name(function(e) {
    let t = e.byobRequest;
    return u(t) ? 0 : w(t);
  }, "__wbg_byobRequest_f8b1c89429b77545"), __wbg_byteLength_336bc7d303511ba0: /* @__PURE__ */ __name(function(e) {
    return e.byteLength;
  }, "__wbg_byteLength_336bc7d303511ba0"), __wbg_byteOffset_2b1d5b10453ce198: /* @__PURE__ */ __name(function(e) {
    return e.byteOffset;
  }, "__wbg_byteOffset_2b1d5b10453ce198"), __wbg_call_35dba3c747ad7521: /* @__PURE__ */ __name(function(e, t, _) {
    return e.call(t, _);
  }, "__wbg_call_35dba3c747ad7521"), __wbg_cancel_9610ed0dbe990e64: /* @__PURE__ */ __name(function(e) {
    return e.cancel();
  }, "__wbg_cancel_9610ed0dbe990e64"), __wbg_catch_8094577c3f159ad5: /* @__PURE__ */ __name(function(e, t) {
    return e.catch(t);
  }, "__wbg_catch_8094577c3f159ad5"), __wbg_cause_c8c260abb3caaebd: /* @__PURE__ */ __name(function(e) {
    return e.cause;
  }, "__wbg_cause_c8c260abb3caaebd"), __wbg_cf_0214917b05eb22e8: /* @__PURE__ */ __name(function(e) {
    let t = e.cf;
    return u(t) ? 0 : w(t);
  }, "__wbg_cf_0214917b05eb22e8"), __wbg_cf_a20029b537f86569: /* @__PURE__ */ __name(function(e) {
    let t = e.cf;
    return u(t) ? 0 : w(t);
  }, "__wbg_cf_a20029b537f86569"), __wbg_close_72f69f5f2de2bc73: /* @__PURE__ */ __name(function(e) {
    e.close();
  }, "__wbg_close_72f69f5f2de2bc73"), __wbg_close_97cdb44c3a7878f6: /* @__PURE__ */ __name(function(e) {
    e.close();
  }, "__wbg_close_97cdb44c3a7878f6"), __wbg_constructor_aaf603909746f45e: /* @__PURE__ */ __name(function(e) {
    return e.constructor;
  }, "__wbg_constructor_aaf603909746f45e"), __wbg_done_669171204c3dcae2: /* @__PURE__ */ __name(function(e) {
    return e.done;
  }, "__wbg_done_669171204c3dcae2"), __wbg_enqueue_7d68a21eda78e72f: /* @__PURE__ */ __name(function(e, t) {
    e.enqueue(t);
  }, "__wbg_enqueue_7d68a21eda78e72f"), __wbg_entries_2c710161cbd65b89: /* @__PURE__ */ __name(function(e) {
    return e.entries();
  }, "__wbg_entries_2c710161cbd65b89"), __wbg_error_afb37ce311f1115d: /* @__PURE__ */ __name(function(e, t) {
    console.error(e, t);
  }, "__wbg_error_afb37ce311f1115d"), __wbg_error_dd408a7b3cb542dd: /* @__PURE__ */ __name(function(e) {
    console.error(e);
  }, "__wbg_error_dd408a7b3cb542dd"), __wbg_fetch_75234d52fb417eb2: /* @__PURE__ */ __name(function(e, t, _) {
    return e.fetch(t, _);
  }, "__wbg_fetch_75234d52fb417eb2"), __wbg_fetch_f155d464163cac10: /* @__PURE__ */ __name(function(e, t, _, i) {
    return e.fetch(h(t, _), i);
  }, "__wbg_fetch_f155d464163cac10"), __wbg_getRandomValues_436a51d0629d84e1: /* @__PURE__ */ __name(function(e, t) {
    globalThis.crypto.getRandomValues(L(e, t));
  }, "__wbg_getRandomValues_436a51d0629d84e1"), __wbg_getReader_bb0230851fbf986b: /* @__PURE__ */ __name(function(e) {
    return e.getReader();
  }, "__wbg_getReader_bb0230851fbf986b"), __wbg_getTime_63fb0332e6c4ec17: /* @__PURE__ */ __name(function(e) {
    return e.getTime();
  }, "__wbg_getTime_63fb0332e6c4ec17"), __wbg_get_6e5000bdfe2fdcdc: /* @__PURE__ */ __name(function(e, t) {
    let _ = Reflect.get(e, t);
    return u(_) ? 0 : w(_);
  }, "__wbg_get_6e5000bdfe2fdcdc"), __wbg_get_971a0c45d172643f: /* @__PURE__ */ __name(function(e, t) {
    return Reflect.get(e, t);
  }, "__wbg_get_971a0c45d172643f"), __wbg_get_c0c8f8d7da0c03dd: /* @__PURE__ */ __name(function(e, t) {
    return e[t >>> 0];
  }, "__wbg_get_c0c8f8d7da0c03dd"), __wbg_get_done_ce5b5691b59c07f2: /* @__PURE__ */ __name(function(e) {
    let t = e.done;
    return u(t) ? 16777215 : t ? 1 : 0;
  }, "__wbg_get_done_ce5b5691b59c07f2"), __wbg_get_value_58309ba057b715e1: /* @__PURE__ */ __name(function(e) {
    return e.value;
  }, "__wbg_get_value_58309ba057b715e1"), __wbg_headers_6dedf39f001ae99d: /* @__PURE__ */ __name(function(e) {
    return e.headers;
  }, "__wbg_headers_6dedf39f001ae99d"), __wbg_headers_92567b07014384b9: /* @__PURE__ */ __name(function(e) {
    return e.headers;
  }, "__wbg_headers_92567b07014384b9"), __wbg_httpProtocol_0eb86107af90610b: /* @__PURE__ */ __name(function(e, t) {
    let _ = t.httpProtocol, i = I(_, r.__wbindgen_malloc, r.__wbindgen_realloc), a = l;
    d().setInt32(e + 4, a, true), d().setInt32(e + 0, i, true);
  }, "__wbg_httpProtocol_0eb86107af90610b"), __wbg_instanceId_27a130234331eb08: /* @__PURE__ */ __name(function(e) {
    return e.instanceId;
  }, "__wbg_instanceId_27a130234331eb08"), __wbg_instanceof_Error_61d8a02a0f3383a1: /* @__PURE__ */ __name(function(e) {
    let t;
    try {
      t = e instanceof Error;
    } catch {
      t = false;
    }
    return t;
  }, "__wbg_instanceof_Error_61d8a02a0f3383a1"), __wbg_instanceof_ReadableStream_92e742420fb1d888: /* @__PURE__ */ __name(function(e) {
    let t;
    try {
      t = e instanceof ReadableStream;
    } catch {
      t = false;
    }
    return t;
  }, "__wbg_instanceof_ReadableStream_92e742420fb1d888"), __wbg_instanceof_Response_8f49efbd4bfd76d6: /* @__PURE__ */ __name(function(e) {
    let t;
    try {
      t = e instanceof Response;
    } catch {
      t = false;
    }
    return t;
  }, "__wbg_instanceof_Response_8f49efbd4bfd76d6"), __wbg_keys_51ac7f90d02b781c: /* @__PURE__ */ __name(function(e) {
    return e.keys();
  }, "__wbg_keys_51ac7f90d02b781c"), __wbg_length_36bd29c6848c2144: /* @__PURE__ */ __name(function(e) {
    return e.length;
  }, "__wbg_length_36bd29c6848c2144"), __wbg_message_c141d5e68716b595: /* @__PURE__ */ __name(function(e) {
    return e.message;
  }, "__wbg_message_c141d5e68716b595"), __wbg_method_c3004ada2c948f17: /* @__PURE__ */ __name(function(e, t) {
    let _ = t.method, i = I(_, r.__wbindgen_malloc, r.__wbindgen_realloc), a = l;
    d().setInt32(e + 4, a, true), d().setInt32(e + 0, i, true);
  }, "__wbg_method_c3004ada2c948f17"), __wbg_minifyconfig_new: /* @__PURE__ */ __name(function(e) {
    return v.__wrap(e);
  }, "__wbg_minifyconfig_new"), __wbg_name_7adfb7f7f1539878: /* @__PURE__ */ __name(function(e) {
    return e.name;
  }, "__wbg_name_7adfb7f7f1539878"), __wbg_name_9c7c033b04304598: /* @__PURE__ */ __name(function(e) {
    return e.name;
  }, "__wbg_name_9c7c033b04304598"), __wbg_new_0_f117d868b403dc07: /* @__PURE__ */ __name(function() {
    return /* @__PURE__ */ new Date();
  }, "__wbg_new_0_f117d868b403dc07"), __wbg_new_358857d90afd5a2d: /* @__PURE__ */ __name(function(e, t) {
    return new Error(h(e, t));
  }, "__wbg_new_358857d90afd5a2d"), __wbg_new_95039e162b0c4466: /* @__PURE__ */ __name(function() {
    return new Headers();
  }, "__wbg_new_95039e162b0c4466"), __wbg_new_cdf041679ded4c5f: /* @__PURE__ */ __name(function() {
    return /* @__PURE__ */ new Map();
  }, "__wbg_new_cdf041679ded4c5f"), __wbg_new_ebe3e0f6837f0879: /* @__PURE__ */ __name(function() {
    return new Object();
  }, "__wbg_new_ebe3e0f6837f0879"), __wbg_new_typed_cceaf62d8d95e9f2: /* @__PURE__ */ __name(function(e, t) {
    try {
      var _ = { a: e, b: t }, i = /* @__PURE__ */ __name((f, b) => {
        let g = _.a;
        _.a = 0;
        try {
          return ce(g, _.b, f, b);
        } finally {
          _.a = g;
        }
      }, "i");
      return new Promise(i);
    } finally {
      _.a = 0;
    }
  }, "__wbg_new_typed_cceaf62d8d95e9f2"), __wbg_new_with_byte_offset_and_length_ff6e927f8d72f0c3: /* @__PURE__ */ __name(function(e, t, _) {
    return new Uint8Array(e, t >>> 0, _ >>> 0);
  }, "__wbg_new_with_byte_offset_and_length_ff6e927f8d72f0c3"), __wbg_new_with_into_underlying_source_8812003c9985c511: /* @__PURE__ */ __name(function(e, t) {
    return new ReadableStream(m.__wrap(e), t);
  }, "__wbg_new_with_into_underlying_source_8812003c9985c511"), __wbg_new_with_length_3ffc1c56427c525c: /* @__PURE__ */ __name(function(e) {
    return new Uint8Array(e >>> 0);
  }, "__wbg_new_with_length_3ffc1c56427c525c"), __wbg_new_with_opt_buffer_source_and_init_ea956d00e72aaeb7: /* @__PURE__ */ __name(function(e, t) {
    return new Response(e, t);
  }, "__wbg_new_with_opt_buffer_source_and_init_ea956d00e72aaeb7"), __wbg_new_with_opt_readable_stream_and_init_1ccadcd958ee9415: /* @__PURE__ */ __name(function(e, t) {
    return new Response(e, t);
  }, "__wbg_new_with_opt_readable_stream_and_init_1ccadcd958ee9415"), __wbg_new_with_opt_str_and_init_79a4cbb0efa211ad: /* @__PURE__ */ __name(function(e, t, _) {
    return new Response(e === 0 ? void 0 : h(e, t), _);
  }, "__wbg_new_with_opt_str_and_init_79a4cbb0efa211ad"), __wbg_new_with_str_and_init_5a37d576dec75a86: /* @__PURE__ */ __name(function(e, t, _) {
    return new Request(h(e, t), _);
  }, "__wbg_new_with_str_and_init_5a37d576dec75a86"), __wbg_next_42cf16ee0dafc9e2: /* @__PURE__ */ __name(function(e) {
    return e.next();
  }, "__wbg_next_42cf16ee0dafc9e2"), __wbg_prototypesetcall_de8e0d9553586985: /* @__PURE__ */ __name(function(e, t, _) {
    Uint8Array.prototype.set.call(L(e, t), _);
  }, "__wbg_prototypesetcall_de8e0d9553586985"), __wbg_queueMicrotask_ac694eae12e92dfb: /* @__PURE__ */ __name(function(e) {
    queueMicrotask(e);
  }, "__wbg_queueMicrotask_ac694eae12e92dfb"), __wbg_queueMicrotask_be5fe34a8f4cad4d: /* @__PURE__ */ __name(function(e) {
    return e.queueMicrotask;
  }, "__wbg_queueMicrotask_be5fe34a8f4cad4d"), __wbg_read_ae34ffedeb11f034: /* @__PURE__ */ __name(function(e) {
    return e.read();
  }, "__wbg_read_ae34ffedeb11f034"), __wbg_redirect_dd9ef803a67cee6b: /* @__PURE__ */ __name(function(e) {
    let t = e.redirect;
    return (G.indexOf(t) + 1 || 4) - 1;
  }, "__wbg_redirect_dd9ef803a67cee6b"), __wbg_releaseLock_f38d2d1c08212a8a: /* @__PURE__ */ __name(function(e) {
    e.releaseLock();
  }, "__wbg_releaseLock_f38d2d1c08212a8a"), __wbg_resolve_020f95d838c6ef25: /* @__PURE__ */ __name(function(e) {
    return Promise.resolve(e);
  }, "__wbg_resolve_020f95d838c6ef25"), __wbg_respond_f88cbcebace42068: /* @__PURE__ */ __name(function(e, t) {
    e.respond(t >>> 0);
  }, "__wbg_respond_f88cbcebace42068"), __wbg_set_014226dfeca53178: /* @__PURE__ */ __name(function(e, t, _) {
    return e.set(t, _);
  }, "__wbg_set_014226dfeca53178"), __wbg_set_6be42768c690e380: /* @__PURE__ */ __name(function(e, t, _) {
    e[t] = _;
  }, "__wbg_set_6be42768c690e380"), __wbg_set_8155bb79a948541b: /* @__PURE__ */ __name(function(e, t, _) {
    return Reflect.set(e, t, _);
  }, "__wbg_set_8155bb79a948541b"), __wbg_set_b9b5b5cb7b495037: /* @__PURE__ */ __name(function(e, t, _) {
    e.set(L(t, _));
  }, "__wbg_set_b9b5b5cb7b495037"), __wbg_set_body_f301b68bff45f419: /* @__PURE__ */ __name(function(e, t) {
    e.body = t;
  }, "__wbg_set_body_f301b68bff45f419"), __wbg_set_cache_ab8f11813716fe29: /* @__PURE__ */ __name(function(e, t) {
    e.cache = fe[t];
  }, "__wbg_set_cache_ab8f11813716fe29"), __wbg_set_criticalError_0d391f2a45c3bc42: /* @__PURE__ */ __name(function(e, t) {
    e.criticalError = t !== 0;
  }, "__wbg_set_criticalError_0d391f2a45c3bc42"), __wbg_set_headers_805555608daf7f2a: /* @__PURE__ */ __name(function(e, t) {
    e.headers = t;
  }, "__wbg_set_headers_805555608daf7f2a"), __wbg_set_headers_f0f86971ae98d262: /* @__PURE__ */ __name(function(e, t) {
    e.headers = t;
  }, "__wbg_set_headers_f0f86971ae98d262"), __wbg_set_high_water_mark_75e3da9cfad3d509: /* @__PURE__ */ __name(function(e, t) {
    e.highWaterMark = t;
  }, "__wbg_set_high_water_mark_75e3da9cfad3d509"), __wbg_set_instanceId_9b3420954adec865: /* @__PURE__ */ __name(function(e, t) {
    e.instanceId = t >>> 0;
  }, "__wbg_set_instanceId_9b3420954adec865"), __wbg_set_method_cf2b992b9a610bc3: /* @__PURE__ */ __name(function(e, t, _) {
    e.method = h(t, _);
  }, "__wbg_set_method_cf2b992b9a610bc3"), __wbg_set_redirect_9d53fb52143d8ea4: /* @__PURE__ */ __name(function(e, t) {
    e.redirect = G[t];
  }, "__wbg_set_redirect_9d53fb52143d8ea4"), __wbg_set_signal_115b9e9423652e66: /* @__PURE__ */ __name(function(e, t) {
    e.signal = t;
  }, "__wbg_set_signal_115b9e9423652e66"), __wbg_set_status_72bec7ae976c21fb: /* @__PURE__ */ __name(function(e, t) {
    e.status = t;
  }, "__wbg_set_status_72bec7ae976c21fb"), __wbg_signal_41db0917e3f6b786: /* @__PURE__ */ __name(function(e) {
    return e.signal;
  }, "__wbg_signal_41db0917e3f6b786"), __wbg_static_accessor_GLOBAL_THIS_466428f93b4eaa76: /* @__PURE__ */ __name(function() {
    let e = typeof globalThis > "u" ? null : globalThis;
    return u(e) ? 0 : w(e);
  }, "__wbg_static_accessor_GLOBAL_THIS_466428f93b4eaa76"), __wbg_static_accessor_GLOBAL_c7aea38d4de089bc: /* @__PURE__ */ __name(function() {
    let e = typeof global > "u" ? null : global;
    return u(e) ? 0 : w(e);
  }, "__wbg_static_accessor_GLOBAL_c7aea38d4de089bc"), __wbg_static_accessor_INIT_STATE_35966a05809b7176: /* @__PURE__ */ __name(function() {
    return H;
  }, "__wbg_static_accessor_INIT_STATE_35966a05809b7176"), __wbg_static_accessor_SELF_42d4fae05e59267a: /* @__PURE__ */ __name(function() {
    let e = typeof self > "u" ? null : self;
    return u(e) ? 0 : w(e);
  }, "__wbg_static_accessor_SELF_42d4fae05e59267a"), __wbg_static_accessor_WINDOW_e0db14a0eba6a812: /* @__PURE__ */ __name(function() {
    let e = typeof window > "u" ? null : window;
    return u(e) ? 0 : w(e);
  }, "__wbg_static_accessor_WINDOW_e0db14a0eba6a812"), __wbg_status_b0de02a07fd7d927: /* @__PURE__ */ __name(function(e) {
    return e.status;
  }, "__wbg_status_b0de02a07fd7d927"), __wbg_then_7026b513a94278a8: /* @__PURE__ */ __name(function(e, t) {
    return e.then(t);
  }, "__wbg_then_7026b513a94278a8"), __wbg_then_72819b8d4e081fb5: /* @__PURE__ */ __name(function(e, t, _) {
    return e.then(t, _);
  }, "__wbg_then_72819b8d4e081fb5"), __wbg_url_3e90676c7072325d: /* @__PURE__ */ __name(function(e, t) {
    let _ = t.url, i = I(_, r.__wbindgen_malloc, r.__wbindgen_realloc), a = l;
    d().setInt32(e + 4, a, true), d().setInt32(e + 0, i, true);
  }, "__wbg_url_3e90676c7072325d"), __wbg_value_1e2369fab29b420e: /* @__PURE__ */ __name(function(e) {
    return e.value;
  }, "__wbg_value_1e2369fab29b420e"), __wbg_view_7685fe4b2845c5b6: /* @__PURE__ */ __name(function(e) {
    let t = e.view;
    return u(t) ? 0 : w(t);
  }, "__wbg_view_7685fe4b2845c5b6"), __wbg_webSocket_d0b2797319952d03: /* @__PURE__ */ __name(function(e) {
    let t = e.webSocket;
    return u(t) ? 0 : w(t);
  }, "__wbg_webSocket_d0b2797319952d03"), __wbindgen_cast_0000000000000001: /* @__PURE__ */ __name(function(e, t) {
    return Y(e, t, oe);
  }, "__wbindgen_cast_0000000000000001"), __wbindgen_cast_0000000000000002: /* @__PURE__ */ __name(function(e, t) {
    return Y(e, t, se);
  }, "__wbindgen_cast_0000000000000002"), __wbindgen_cast_0000000000000003: /* @__PURE__ */ __name(function(e) {
    return e;
  }, "__wbindgen_cast_0000000000000003"), __wbindgen_cast_0000000000000004: /* @__PURE__ */ __name(function(e, t) {
    return h(e, t);
  }, "__wbindgen_cast_0000000000000004"), __wbindgen_cast_0000000000000005: /* @__PURE__ */ __name(function(e) {
    return BigInt.asUintN(64, e);
  }, "__wbindgen_cast_0000000000000005"), __wbindgen_init_externref_table: /* @__PURE__ */ __name(function() {
    let e = r.__wbindgen_externrefs, t = e.grow(4);
    e.set(0, void 0), e.set(t + 0, void 0), e.set(t + 1, null), e.set(t + 2, true), e.set(t + 3, false);
  }, "__wbindgen_init_externref_table"), __wbindgen_jstag: WebAssembly.JSTag, __wbindgen_rethrow_critical: /* @__PURE__ */ __name(function(e) {
    throw new Error("Critical error", { cause: e });
  }, "__wbindgen_rethrow_critical") } };
}
__name(_e, "_e");
var V = new WebAssembly.Tag({ parameters: ["externref"] });
var C;
var q = false;
function re() {
  q = true;
  try {
    let n3 = B()[r.__abort_handler.value / 4];
    n3 && r.__wbindgen_export.get(n3)();
  } catch {
  }
}
__name(re, "re");
function o(n3) {
  throw n3 instanceof WebAssembly.Exception && n3.is(V) ? n3.getArg(V, 0) : (B()[C] = 1, re(), n3);
}
__name(o, "o");
function c() {
  if (C ??= r.__instance_terminated.value / 4, B()[C]) {
    if (q || re(), z) {
      T();
      return;
    }
    throw new Error("Module terminated");
  } else z && T();
}
__name(c, "c");
function se(n3, e, t) {
  c();
  try {
    r.wasm_bindgen_47db854921326dcc___convert__closures_____invoke___wasm_bindgen_47db854921326dcc___JsValue______true_(n3, e, t);
  } catch (_) {
    o(_);
  }
}
__name(se, "se");
function oe(n3, e, t) {
  let _;
  c();
  try {
    _ = r.wasm_bindgen_47db854921326dcc___convert__closures_____invoke___wasm_bindgen_47db854921326dcc___JsValue__core_9b3796e30d99ddb7___result__Result_____wasm_bindgen_47db854921326dcc___JsError___true_(n3, e, t);
  } catch (i) {
    o(i);
  }
  if (_[1]) throw he(_[0]);
}
__name(oe, "oe");
function ce(n3, e, t, _) {
  c();
  try {
    r.wasm_bindgen_47db854921326dcc___convert__closures_____invoke___js_sys_eece81ba840fea30___Function_fn_wasm_bindgen_47db854921326dcc___JsValue_____wasm_bindgen_47db854921326dcc___sys__Undefined___js_sys_eece81ba840fea30___Function_fn_wasm_bindgen_47db854921326dcc___JsValue_____wasm_bindgen_47db854921326dcc___sys__Undefined_______true_(n3, e, t, _);
  } catch (i) {
    o(i);
  }
}
__name(ce, "ce");
var ae = ["bytes"];
var fe = ["default", "no-store", "reload", "no-cache", "force-cache", "only-if-cached"];
var G = ["follow", "error", "manual"];
var s = 0;
var ue = typeof FinalizationRegistry > "u" ? { register: /* @__PURE__ */ __name(() => {
}, "register"), unregister: /* @__PURE__ */ __name(() => {
}, "unregister") } : new FinalizationRegistry(({ ptr: n3, instance: e }) => {
  e === s && r.__wbg_containerstartupoptions_free(n3, 1);
});
var be = typeof FinalizationRegistry > "u" ? { register: /* @__PURE__ */ __name(() => {
}, "register"), unregister: /* @__PURE__ */ __name(() => {
}, "unregister") } : new FinalizationRegistry(({ ptr: n3, instance: e }) => {
  e === s && r.__wbg_intounderlyingbytesource_free(n3, 1);
});
var de = typeof FinalizationRegistry > "u" ? { register: /* @__PURE__ */ __name(() => {
}, "register"), unregister: /* @__PURE__ */ __name(() => {
}, "unregister") } : new FinalizationRegistry(({ ptr: n3, instance: e }) => {
  e === s && r.__wbg_intounderlyingsink_free(n3, 1);
});
var K = typeof FinalizationRegistry > "u" ? { register: /* @__PURE__ */ __name(() => {
}, "register"), unregister: /* @__PURE__ */ __name(() => {
}, "unregister") } : new FinalizationRegistry(({ ptr: n3, instance: e }) => {
  e === s && r.__wbg_intounderlyingsource_free(n3, 1);
});
var Q = typeof FinalizationRegistry > "u" ? { register: /* @__PURE__ */ __name(() => {
}, "register"), unregister: /* @__PURE__ */ __name(() => {
}, "unregister") } : new FinalizationRegistry(({ ptr: n3, instance: e }) => {
  e === s && r.__wbg_minifyconfig_free(n3, 1);
});
var we = typeof FinalizationRegistry > "u" ? { register: /* @__PURE__ */ __name(() => {
}, "register"), unregister: /* @__PURE__ */ __name(() => {
}, "unregister") } : new FinalizationRegistry(({ ptr: n3, instance: e }) => {
  e === s && r.__wbg_r2range_free(n3, 1);
});
function w(n3) {
  let e = r.__externref_table_alloc();
  return r.__wbindgen_externrefs.set(e, n3), e;
}
__name(w, "w");
var X = typeof FinalizationRegistry > "u" ? { register: /* @__PURE__ */ __name(() => {
}, "register"), unregister: /* @__PURE__ */ __name(() => {
}, "unregister") } : new FinalizationRegistry((n3) => {
  n3.instance === s && r.__wbindgen_destroy_closure(n3.a, n3.b);
});
function J(n3) {
  let e = typeof n3;
  if (e == "number" || e == "boolean" || n3 == null) return `${n3}`;
  if (e == "string") return `"${n3}"`;
  if (e == "symbol") {
    let i = n3.description;
    return i == null ? "Symbol" : `Symbol(${i})`;
  }
  if (e == "function") {
    let i = n3.name;
    return typeof i == "string" && i.length > 0 ? `Function(${i})` : "Function";
  }
  if (Array.isArray(n3)) {
    let i = n3.length, a = "[";
    i > 0 && (a += J(n3[0]));
    for (let f = 1; f < i; f++) a += ", " + J(n3[f]);
    return a += "]", a;
  }
  let t = /\[object ([^\]]+)\]/.exec(toString.call(n3)), _;
  if (t && t.length > 1) _ = t[1];
  else return toString.call(n3);
  if (_ == "Object") try {
    return "Object(" + JSON.stringify(n3) + ")";
  } catch {
    return "Object";
  }
  return n3 instanceof Error ? `${n3.name}: ${n3.message}
${n3.stack}` : _;
}
__name(J, "J");
function ge(n3, e) {
  n3 = n3 >>> 0;
  let t = d(), _ = [];
  for (let i = n3; i < n3 + 4 * e; i += 4) _.push(r.__wbindgen_externrefs.get(t.getUint32(i, true)));
  return r.__externref_drop_slice(n3, e), _;
}
__name(ge, "ge");
function L(n3, e) {
  return n3 = n3 >>> 0, A().subarray(n3 / 1, n3 / 1 + e);
}
__name(L, "L");
var y = null;
function d() {
  return (y === null || y.buffer.detached === true || y.buffer.detached === void 0 && y.buffer !== r.memory.buffer) && (y = new DataView(r.memory.buffer)), y;
}
__name(d, "d");
var W = null;
function B() {
  return (W === null || W.byteLength === 0) && (W = new Int32Array(r.memory.buffer)), W;
}
__name(B, "B");
function h(n3, e) {
  return pe(n3 >>> 0, e);
}
__name(h, "h");
var k = null;
function A() {
  return (k === null || k.byteLength === 0) && (k = new Uint8Array(r.memory.buffer)), k;
}
__name(A, "A");
function u(n3) {
  return n3 == null;
}
__name(u, "u");
function Y(n3, e, t) {
  let _ = { a: n3, b: e, cnt: 1, instance: s }, i = /* @__PURE__ */ __name((...a) => {
    if (_.instance !== s) throw new Error("Cannot invoke closure from previous WASM instance");
    _.cnt++;
    let f = _.a;
    _.a = 0;
    try {
      return t(f, _.b, ...a);
    } finally {
      _.a = f, i._wbg_cb_unref();
    }
  }, "i");
  return i._wbg_cb_unref = () => {
    --_.cnt === 0 && (r.__wbindgen_destroy_closure(_.a, _.b), _.a = 0, X.unregister(_));
  }, X.register(i, _, _), i;
}
__name(Y, "Y");
function le(n3, e) {
  let t = e(n3.length * 4, 4) >>> 0;
  for (let _ = 0; _ < n3.length; _++) {
    let i = w(n3[_]);
    d().setUint32(t + 4 * _, i, true);
  }
  return l = n3.length, t;
}
__name(le, "le");
function I(n3, e, t) {
  if (t === void 0) {
    let b = P.encode(n3), g = e(b.length, 1) >>> 0;
    return A().subarray(g, g + b.length).set(b), l = b.length, g;
  }
  let _ = n3.length, i = e(_, 1) >>> 0, a = A(), f = 0;
  for (; f < _; f++) {
    let b = n3.charCodeAt(f);
    if (b > 127) break;
    a[i + f] = b;
  }
  if (f !== _) {
    f !== 0 && (n3 = n3.slice(f)), i = t(i, _, _ = f + n3.length * 3, 1) >>> 0;
    let b = A().subarray(i + f, i + _), g = P.encodeInto(n3, b);
    f += g.written, i = t(i, _, f, 1) >>> 0;
  }
  return l = f, i;
}
__name(I, "I");
var z = false;
function he(n3) {
  let e = r.__wbindgen_externrefs.get(n3);
  return r.__externref_table_dealloc(n3), e;
}
__name(he, "he");
var ie = new TextDecoder("utf-8", { ignoreBOM: true, fatal: true });
ie.decode();
function pe(n3, e) {
  return ie.decode(A().subarray(n3, n3 + e));
}
__name(pe, "pe");
var P = new TextEncoder();
"encodeInto" in P || (P.encodeInto = function(n3, e) {
  let t = P.encode(n3);
  return e.set(t), { read: n3.length, written: t.length };
});
var l = 0;
var D = new WebAssembly.Instance(Z, _e());
var r = D.exports;
r.__wbindgen_start();
Error.stackTraceLimit = 100;
var p = ee();
function N() {
  p.criticalError && (console.log("Reinitializing Wasm application"), T(), p.criticalError = false, p.instanceId++);
}
__name(N, "N");
addEventListener("error", (n3) => {
  $(n3.error);
});
function $(n3) {
  n3 instanceof WebAssembly.RuntimeError && (console.error("Critical", n3), p.criticalError = true);
}
__name($, "$");
var O = class extends me {
  static {
    __name(this, "O");
  }
};
O.prototype.fetch = function(e) {
  return te.call(this, e, this.env, this.ctx);
};
O.prototype.init = ne;
var ve = { set: /* @__PURE__ */ __name((n3, e, t, _) => Reflect.set(n3.instance, e, t, _), "set"), has: /* @__PURE__ */ __name((n3, e) => Reflect.has(n3.instance, e), "has"), deleteProperty: /* @__PURE__ */ __name((n3, e) => Reflect.deleteProperty(n3.instance, e), "deleteProperty"), apply: /* @__PURE__ */ __name((n3, e, t) => Reflect.apply(n3.instance, e, t), "apply"), construct: /* @__PURE__ */ __name((n3, e, t) => Reflect.construct(n3.instance, e, t), "construct"), getPrototypeOf: /* @__PURE__ */ __name((n3) => Reflect.getPrototypeOf(n3.instance), "getPrototypeOf"), setPrototypeOf: /* @__PURE__ */ __name((n3, e) => Reflect.setPrototypeOf(n3.instance, e), "setPrototypeOf"), isExtensible: /* @__PURE__ */ __name((n3) => Reflect.isExtensible(n3.instance), "isExtensible"), preventExtensions: /* @__PURE__ */ __name((n3) => Reflect.preventExtensions(n3.instance), "preventExtensions"), getOwnPropertyDescriptor: /* @__PURE__ */ __name((n3, e) => Reflect.getOwnPropertyDescriptor(n3.instance, e), "getOwnPropertyDescriptor"), defineProperty: /* @__PURE__ */ __name((n3, e, t) => Reflect.defineProperty(n3.instance, e, t), "defineProperty"), ownKeys: /* @__PURE__ */ __name((n3) => Reflect.ownKeys(n3.instance), "ownKeys") };
var x = { construct(n3, e, t) {
  try {
    N();
    let _ = { instance: Reflect.construct(n3, e, t), instanceId: p.instanceId, ctor: n3, args: e, newTarget: t };
    return new Proxy(_, { ...ve, get(i, a, f) {
      i.instanceId !== p.instanceId && (i.instance = Reflect.construct(i.ctor, i.args, i.newTarget), i.instanceId = p.instanceId);
      let b = Reflect.get(i.instance, a, f);
      return typeof b != "function" ? b : b.constructor === Function ? new Proxy(b, { apply(g, M, U) {
        N();
        try {
          return g.apply(M, U);
        } catch (S) {
          throw $(S), S;
        }
      } }) : new Proxy(b, { async apply(g, M, U) {
        N();
        try {
          return await g.apply(M, U);
        } catch (S) {
          throw $(S), S;
        }
      } });
    } });
  } catch (_) {
    throw p.criticalError = true, _;
  }
} };
var je = new Proxy(O, x);
var Fe = new Proxy(E, x);
var Se = new Proxy(R, x);
var We = new Proxy(j, x);
var ke = new Proxy(m, x);
var Ae = new Proxy(v, x);
var Pe = new Proxy(F, x);
export {
  Fe as ContainerStartupOptions,
  Se as IntoUnderlyingByteSource,
  We as IntoUnderlyingSink,
  ke as IntoUnderlyingSource,
  Ae as MinifyConfig,
  Pe as R2Range,
  je as default
};
//# sourceMappingURL=index.js.map
