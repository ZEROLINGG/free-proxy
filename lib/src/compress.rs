//lib/src/compress.rs
use anyhow::{Result, anyhow};

const MAX_COMPRESSION_RATIO: u64 = 1024;
const MAX_DECOMPRESSED_SIZE: u64 = 256 * 1024 * 1024; // 256 MiB
const MAX_PREALLOC: usize = 8 * 1024 * 1024; // 8 MiB
const ZSTD_WINDOW_LOG_MAX: u32 = 27; // 128 MiB window

pub trait Compressor {
    fn compress<T: AsRef<[u8]>>(input: T) -> Result<Vec<u8>>;
    fn decompress(input: &[u8]) -> Result<Vec<u8>>;
}

fn bomb_limit_for(input_len: usize) -> u64 {
    (input_len as u64)
        .saturating_mul(MAX_COMPRESSION_RATIO)
        .min(MAX_DECOMPRESSED_SIZE)
}

fn safe_prealloc_cap(declared: Option<u64>, fallback_input_len: usize) -> usize {
    match declared {
        Some(size) => (size as usize).min(MAX_PREALLOC),
        None => fallback_input_len.saturating_mul(3).min(MAX_PREALLOC),
    }
}

// ====================== Lz4 ======================

pub struct Lz4;

impl Compressor for Lz4 {
    fn compress<T: AsRef<[u8]>>(input: T) -> Result<Vec<u8>> {
        Ok(lz4_flex::compress_prepend_size(input.as_ref()))
    }

    fn decompress(input: &[u8]) -> Result<Vec<u8>> {
        if input.len() < 4 {
            return Err(anyhow!("Input too short to contain LZ4 size prefix"));
        }

        let declared_size = u32::from_le_bytes(input[..4].try_into()?) as u64;
        let bomb_limit = bomb_limit_for(input.len());

        if declared_size > bomb_limit {
            return Err(anyhow!(
                "LZ4 decompression bomb detected: declared size {} exceeds limit {}",
                declared_size,
                bomb_limit
            ));
        }

        let out = lz4_flex::decompress_size_prepended(input)?;

        if out.len() as u64 > bomb_limit {
            return Err(anyhow!("LZ4 decompression bomb detected during decode"));
        }

        Ok(out)
    }
}


// ====================== Zstd ======================

pub struct Zstd;

impl Compressor for Zstd {
    fn compress<T: AsRef<[u8]>>(input: T) -> Result<Vec<u8>> {
        Ok(zstd::encode_all(input.as_ref(), 5)?)
    }

    fn decompress(input: &[u8]) -> Result<Vec<u8>> {
        use std::io::Read;

        let bomb_limit = bomb_limit_for(input.len());

        let declared_size = zstd::zstd_safe::get_frame_content_size(input)
            .ok()
            .flatten();

        if let Some(size) = declared_size
            && size > bomb_limit
        {
            return Err(anyhow!(
                "Zstd decompression bomb detected via frame header: {} exceeds limit {}",
                size,
                bomb_limit
            ));
        }

        let estimated_cap = safe_prealloc_cap(declared_size, input.len());

        let mut buf = Vec::with_capacity(estimated_cap);
        let mut decoder = zstd::Decoder::new(input)?;

        decoder
            .window_log_max(ZSTD_WINDOW_LOG_MAX)
            .map_err(|e| anyhow!("failed to set zstd window log max: {e}"))?;

        let mut limited_reader = decoder.take(bomb_limit + 1);

        limited_reader.read_to_end(&mut buf)?;

        if buf.len() as u64 > bomb_limit {
            return Err(anyhow!("Zstd decompression bomb detected during read"));
        }

        Ok(buf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLAINTEXT: &str = r#"<!doctype html><html lang="zh-CN" dir="ltr"><head><base href="https://translate.google.com/"><link rel="preconnect" href="//www.gstatic.com"><meta name="referrer" content="origin"><script nonce="Q6h-6INOSqtdlu1mLAcGhw">window['ppConfig'] = {productName: 'TranslateWebserverUi', deleteIsEnforced:  true , sealIsEnforced:  true , heartbeatRate:  0.5 , periodicReportingRateMillis:  60000.0 , disableAllReporting:  false };(function(){'use strict';function k(a){var b=0;return function(){return b<a.length?{done:!1,value:a[b++]}:{done:!0}}}function l(a){var b=typeof Symbol!="undefined"&&Symbol.iterator&&a[Symbol.iterator];if(b)return b.call(a);if(typeof a.length=="number")return{next:k(a)};throw Error(String(a)+" is not an iterable or ArrayLike");}var m=typeof Object.defineProperties=="function"?Object.defineProperty:function(a,b,c){if(a==Array.prototype||a==Object.prototype)return a;a[b]=c.value;return a};
function n(a){a=["object"==typeof globalThis&&globalThis,a,"object"==typeof window&&window,"object"==typeof self&&self,"object"==typeof global&&global];for(var b=0;b<a.length;++b){var c=a[b];if(c&&c.Math==Math)return c}throw Error("Cannot find global object");}var p=n(this);function q(a,b){if(b)a:{var c=p;a=a.split(".");for(var d=0;d<a.length-1;d++){var e=a[d];if(!(e in c))break a;c=c[e]}a=a[a.length-1];d=c[a];b=b(d);b!=d&&b!=null&&m(c,a,{configurable:!0,writable:!0,value:b})}}
q("Object.is",function(a){return a?a:function(b,c){return b===c?b!==0||1/b===1/c:b!==b&&c!==c}});q("Array.prototype.includes",function(a){return a?a:function(b,c){var d=this;d instanceof String&&(d=String(d));var e=d.length;c=c||0;for(c<0&&(c=Math.max(c+e,0));c<e;c++){var f=d[c];if(f===b||Object.is(f,b))return!0}return!1}});
q("String.prototype.includes",function(a){return a?a:function(b,c){if(this==null)throw new TypeError("The 'this' value for String.prototype.includes must not be null or undefined");if(b instanceof RegExp)throw new TypeError("First argument to String.prototype.includes must not be a regular expression");return this.indexOf(b,c||0)!==-1}});function r(a,b,c){a("https://csp.withgoogle.com/csp/proto/"+encodeURIComponent(b),JSON.stringify(c))}function t(){var a;if((a=window.ppConfig)==null?0:a.disableAllReporting)return function(){};var b,c,d,e;return(e=(b=window)==null?void 0:(c=b.navigator)==null?void 0:(d=c.sendBeacon)==null?void 0:d.bind(navigator))!=null?e:u}function u(a,b){var c=new XMLHttpRequest;c.open("POST",a);c.send(b)}
function v(){var a=(w=Object.prototype)==null?void 0:w.__lookupGetter__("__proto__"),b=x,c=y;return function(){var d=a.call(this),e,f,g,h;r(c,b,{type:"ACCESS_GET",origin:(f=window.location.origin)!=null?f:"unknown",report:{className:(g=d==null?void 0:(e=d.constructor)==null?void 0:e.name)!=null?g:"unknown",stackTrace:(h=Error().stack)!=null?h:"unknown"}});return d}}
function z(){var a=(A=Object.prototype)==null?void 0:A.__lookupSetter__("__proto__"),b=x,c=y;return function(d){d=a.call(this,d);var e,f,g,h;r(c,b,{type:"ACCESS_SET",origin:(f=window.location.origin)!=null?f:"unknown",report:{className:(g=d==null?void 0:(e=d.constructor)==null?void 0:e.name)!=null?g:"unknown",stackTrace:(h=Error().stack)!=null?h:"unknown"}});return d}}function B(a,b){C(a.productName,b);setInterval(function(){C(a.productName,b)},a.periodicReportingRateMillis)}
var D="constructor __defineGetter__ __defineSetter__ hasOwnProperty __lookupGetter__ __lookupSetter__ isPrototypeOf propertyIsEnumerable toString valueOf __proto__ toLocaleString x_ngfn_x".split(" "),E=D.concat,G=navigator.userAgent.match(/Firefox\/([0-9]+)\./),H=(!G||G.length<2?0:Number(G[1])<75)?["toSource"]:[],I;if(H instanceof Array)I=H;else{for(var J=l(H),K,L=[];!(K=J.next()).done;)L.push(K.value);I=L}var M=E.call(D,I),N=[];
function C(a,b){var c=[],d=l(Object.getOwnPropertyNames(Object.prototype)),e=d.next(),f;try{for(;!e.done;e=d.next()){var g=e.value;M.includes(g)||N.includes(g)||c.push(g)}}finally{e&&!e.done&&(f=d.return)&&f.call(d)}e=Object.prototype;d=[];for(f=0;f<c.length;f++)g=c[f],d[f]={name:g,descriptor:Object.getOwnPropertyDescriptor(Object.prototype,g),type:typeof e[g]};if(d.length!==0){c=l(d);e=c.next();var h;try{for(;!e.done;e=c.next())N.push(e.value.name)}finally{e&&!e.done&&(h=c.return)&&h.call(c)}var F;
r(b,a,{type:"SEAL",origin:(F=window.location.origin)!=null?F:"unknown",report:{blockers:d}})}};var O=Math.random(),P=t(),Q=window.ppConfig;Q&&(Q.disableAllReporting||Q.deleteIsEnforced&&Q.sealIsEnforced||O<Q.heartbeatRate&&r(P,Q.productName,{origin:window.location.origin,type:"HEARTBEAT"}));var y=t(),R=window.ppConfig;if(R)if(R.deleteIsEnforced)delete Object.prototype.__proto__;else if(!R.disableAllReporting){var x=R.productName;try{var w,A;Object.defineProperty(Object.prototype,"__proto__",{enumerable:!1,get:v(),set:z()})}catch(a){}}
(function(){var a=t(),b=window.ppConfig;b&&(b.sealIsEnforced?Object.seal(Object.prototype):b.disableAllReporting||(document.readyState!=="loading"?B(b,a):document.addEventListener("DOMContentLoaded",function(){B(b,a)})))})();}).call(this);"#;


    #[test]
    fn test_lz4_roundtrip() {
        let compressed = Lz4::compress(PLAINTEXT).unwrap();
        let decompressed = Lz4::decompress(&compressed).unwrap();
        assert_eq!(PLAINTEXT, String::from_utf8(decompressed).unwrap());
    }

    #[test]
    fn test_zstd_roundtrip() {
        let compressed = Zstd::compress(PLAINTEXT).unwrap();
        let decompressed = Zstd::decompress(&compressed).unwrap();
        assert_eq!(PLAINTEXT, String::from_utf8(decompressed).unwrap());
    }

    #[test]
    fn test_empty_input_roundtrip() {
        let empty_data: &[u8] = b"";

        let lz4_comp = Lz4::compress(empty_data).unwrap();
        assert_eq!(empty_data, &Lz4::decompress(&lz4_comp).unwrap()[..]);

        let zstd_comp = Zstd::compress(empty_data).unwrap();
        assert_eq!(empty_data, &Zstd::decompress(&zstd_comp).unwrap()[..]);
    }

    #[test]
    fn test_lz4_too_short_input() {
        let short_data = b"123";
        assert!(Lz4::decompress(short_data).is_err());
    }

    #[test]
    fn test_zstd_invalid_data() {
        let invalid_data = b"not a valid zstd frame data";
        assert!(Zstd::decompress(invalid_data).is_err());
    }


    #[test]
    fn test_lz4_header_bomb_prevention() {
        let mut compressed = Lz4::compress(b"small data").unwrap();
        let fake_size: u32 = 100 * 1024 * 1024;
        compressed[0..4].copy_from_slice(&fake_size.to_le_bytes());

        let err = Lz4::decompress(&compressed).unwrap_err();
        assert!(err.to_string().contains("LZ4 decompression bomb detected: declared size"));
    }

    #[test]
    fn test_zstd_highly_compressible_bomb_prevention() {
        let huge_zero_data = vec![0u8; 1024 * 1024];
        let compressed = Zstd::compress(&huge_zero_data).unwrap();

        assert!(compressed.len() < 200, "Compressed size is {}", compressed.len());

        let err = Zstd::decompress(&compressed).unwrap_err();
        assert!(
            err.to_string().contains("bomb detected via frame header") ||
                err.to_string().contains("bomb detected during read"),
            "Expected bomb detection error, got: {}", err
        );
    }
}